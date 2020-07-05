use std::{io, path::Path};

use v4l::capture::{Device as CaptureDevice, Format as CaptureFormat};
use v4l::control::{MenuItem as ControlMenuItem, Type as ControlType};
use v4l::DeviceList;
use v4l::FourCC as FourCC_;

use ffimage::packed::DynamicImageView;

use crate::control;
use crate::device::{ControlInfo, FormatInfo, Info as DeviceInfo};
use crate::format::{Format, FourCC};
use crate::hal::traits::Device;
use crate::hal::v4l2::stream::PlatformStream;
use crate::traits::Stream;

pub(crate) struct PlatformList {}

impl PlatformList {
    pub fn enumerate() -> Vec<DeviceInfo> {
        let mut list = Vec::new();
        let platform_list = DeviceList::new();

        for dev in platform_list {
            let index = dev.index();
            let name = dev.name();
            let caps = dev.query_caps();
            if index.is_none() || name.is_none() || caps.is_err() {
                continue;
            }

            let index = index.unwrap();
            let name = name.unwrap();
            let caps = caps.unwrap();

            // For now, require video capture and streaming capabilities.
            // Very old devices may only support the read() I/O mechanism, so support for those
            // might be added in the future. Every recent (released during the last ten to twenty
            // years) webcam should support streaming though.
            let capture_flag = v4l::capability::Flags::VIDEO_CAPTURE;
            let streaming_flag = v4l::capability::Flags::STREAMING;
            if caps.capabilities & capture_flag != capture_flag
                || caps.capabilities & streaming_flag != streaming_flag
            {
                continue;
            }

            let mut controls = Vec::new();
            let plat_controls = dev.query_controls();
            if plat_controls.is_err() {
                continue;
            }

            for control in plat_controls.unwrap() {
                let mut repr = control::Representation::Unknown;
                match control.typ {
                    ControlType::Integer | ControlType::Integer64 => {
                        let constraints = control::Integer {
                            range: (control.minimum as i64, control.maximum as i64),
                            step: control.step as u64,
                            default: control.default as i64,
                        };
                        repr = control::Representation::Integer(constraints);
                    }
                    ControlType::Boolean => {
                        repr = control::Representation::Boolean;
                    }
                    ControlType::Menu => {
                        let mut items = Vec::new();
                        if let Some(plat_items) = control.items {
                            for plat_item in plat_items {
                                match plat_item.1 {
                                    ControlMenuItem::Name(name) => {
                                        items.push(control::MenuItem::String(name));
                                    }
                                    ControlMenuItem::Value(value) => {
                                        items.push(control::MenuItem::Integer(value));
                                    }
                                }
                            }
                        }
                        repr = control::Representation::Menu(items);
                    }
                    ControlType::Button => {
                        repr = control::Representation::Button;
                    }
                    ControlType::String => {
                        repr = control::Representation::String;
                    }
                    ControlType::Bitmask => {
                        repr = control::Representation::Bitmask;
                    }
                    _ => {}
                }

                controls.push(ControlInfo {
                    id: control.id,
                    name: control.name,
                    repr,
                })
            }

            let mut formats = Vec::new();
            let dev = PlatformDevice::new(index);
            if dev.is_err() {
                continue;
            }

            let dev = dev.unwrap();
            let plat_formats = dev.inner.enumerate_formats();
            if plat_formats.is_err() {
                continue;
            }

            for format in plat_formats.unwrap() {
                let plat_sizes = dev.inner.enumerate_framesizes(format.fourcc);
                if plat_sizes.is_err() {
                    continue;
                }
                let mut info = FormatInfo {
                    fourcc: FourCC::new(&format.fourcc.repr),
                    resolutions: Vec::new(),
                    emulated: format.flags & v4l::format::Flags::EMULATED
                        == v4l::format::Flags::EMULATED,
                };
                for plat_size in plat_sizes.unwrap() {
                    // TODO: consider stepwise formats
                    if let v4l::framesize::FrameSizeEnum::Discrete(size) = plat_size.size {
                        info.resolutions.push((size.width, size.height));
                    }
                }
                formats.push(info);
            }

            list.push(DeviceInfo {
                index: index as u32,
                name,
                formats,
                controls,
            })
        }

        list
    }
}

pub(crate) struct PlatformDevice {
    inner: CaptureDevice,
}

impl PlatformDevice {
    pub fn new(index: usize) -> io::Result<Self> {
        let dev = PlatformDevice {
            inner: CaptureDevice::new(index)?,
        };
        Ok(dev)
    }

    pub fn with_path<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let dev = PlatformDevice {
            inner: CaptureDevice::with_path(path)?,
        };
        Ok(dev)
    }

    pub fn inner(&self) -> &CaptureDevice {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut CaptureDevice {
        &mut self.inner
    }
}

impl Device for PlatformDevice {
    fn get_format(&mut self) -> io::Result<Format> {
        let fmt = self.inner.get_format()?;
        Ok(Format::with_stride(
            fmt.width,
            fmt.height,
            FourCC::new(&fmt.fourcc.repr),
            fmt.stride as usize,
        ))
    }

    fn set_format(&mut self, fmt: &Format) -> io::Result<Format> {
        let fmt = CaptureFormat::new(fmt.width, fmt.height, FourCC_::new(&fmt.fourcc.repr));
        self.inner.set_format(&fmt)?;
        self.get_format()
    }

    fn stream<'a>(&'a mut self) -> io::Result<Box<dyn Stream<Item = DynamicImageView> + 'a>> {
        let stream = PlatformStream::new(self)?;
        Ok(Box::new(stream))
    }
}
