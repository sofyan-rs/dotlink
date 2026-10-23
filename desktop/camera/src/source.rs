//! The media source the Windows Camera Frame Server loads for "Dotlink Camera" (Windows 11's
//! MFCreateVirtualCamera). Shaped after Microsoft's virtual camera sample and smourier's
//! VCamSample (MIT):
//!
//! - `Activator` (IMFActivate): what the class factory hands out; creates the source.
//! - `MediaSource` (IMFMediaSourceEx + IMFGetService + IKsControl + IMFSampleAllocatorControl,
//!   and its own attributes): one stream, live.
//! - `MediaStream` (IMFMediaStream2 + IKsControl + attributes): NV12 at a fixed size and 30 fps.
//!   Frames come from Dotlink through the shared file ([crate::shared]); a dark frame stands in
//!   until it has written one.
//!
//! RequestSample only queues the request; a worker thread delivers each sample on the next
//! 30 fps tick, so the frame server gets a steady rate without being blocked.

use crate::shared::{self, FrameReader};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::Media::KernelStreaming::*;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::core::*;

const FPS: u32 = 30;
const DEFAULT_SIZE: (u32, u32) = (1280, 720);
/// HRESULT_FROM_WIN32(ERROR_SET_NOT_FOUND): "no such property set", for every IKsControl call.
const SET_NOT_FOUND: HRESULT = HRESULT(0x8007_0492_u32 as i32);

/// Forwards IMFAttributes to an inner attribute store (the source, stream and activator are
/// all attribute stores too).
macro_rules! attributes_via {
    ($impl:ty, $field:ident) => {
        impl IMFAttributes_Impl for $impl {
            fn GetItem(&self, k: *const GUID, v: *mut PROPVARIANT) -> Result<()> {
                unsafe { self.$field.GetItem(k, Some(v)) }
            }
            fn GetItemType(&self, k: *const GUID) -> Result<MF_ATTRIBUTE_TYPE> {
                unsafe { self.$field.GetItemType(k) }
            }
            fn CompareItem(&self, k: *const GUID, v: *const PROPVARIANT) -> Result<BOOL> {
                unsafe { self.$field.CompareItem(k, v) }
            }
            fn Compare(
                &self,
                theirs: Ref<IMFAttributes>,
                t: MF_ATTRIBUTES_MATCH_TYPE,
            ) -> Result<BOOL> {
                unsafe { self.$field.Compare(theirs.ok()?, t) }
            }
            fn GetUINT32(&self, k: *const GUID) -> Result<u32> {
                unsafe { self.$field.GetUINT32(k) }
            }
            fn GetUINT64(&self, k: *const GUID) -> Result<u64> {
                unsafe { self.$field.GetUINT64(k) }
            }
            fn GetDouble(&self, k: *const GUID) -> Result<f64> {
                unsafe { self.$field.GetDouble(k) }
            }
            fn GetGUID(&self, k: *const GUID) -> Result<GUID> {
                unsafe { self.$field.GetGUID(k) }
            }
            fn GetStringLength(&self, k: *const GUID) -> Result<u32> {
                unsafe { self.$field.GetStringLength(k) }
            }
            fn GetString(&self, k: *const GUID, v: PWSTR, size: u32, len: *mut u32) -> Result<()> {
                unsafe {
                    (Interface::vtable(&self.$field).GetString)(
                        Interface::as_raw(&self.$field),
                        k,
                        v,
                        size,
                        len,
                    )
                    .ok()
                }
            }
            fn GetAllocatedString(
                &self,
                k: *const GUID,
                v: *mut PWSTR,
                len: *mut u32,
            ) -> Result<()> {
                unsafe { self.$field.GetAllocatedString(k, v, len) }
            }
            fn GetBlobSize(&self, k: *const GUID) -> Result<u32> {
                unsafe { self.$field.GetBlobSize(k) }
            }
            fn GetBlob(
                &self,
                k: *const GUID,
                buf: *mut u8,
                size: u32,
                len: *mut u32,
            ) -> Result<()> {
                unsafe {
                    (Interface::vtable(&self.$field).GetBlob)(
                        Interface::as_raw(&self.$field),
                        k,
                        buf,
                        size,
                        len,
                    )
                    .ok()
                }
            }
            fn GetAllocatedBlob(
                &self,
                k: *const GUID,
                buf: *mut *mut u8,
                size: *mut u32,
            ) -> Result<()> {
                unsafe { self.$field.GetAllocatedBlob(k, buf, size) }
            }
            fn GetUnknown(
                &self,
                k: *const GUID,
                riid: *const GUID,
                ppv: *mut *mut core::ffi::c_void,
            ) -> Result<()> {
                unsafe {
                    (Interface::vtable(&self.$field).GetUnknown)(
                        Interface::as_raw(&self.$field),
                        k,
                        riid,
                        ppv,
                    )
                    .ok()
                }
            }
            fn SetItem(&self, k: *const GUID, v: *const PROPVARIANT) -> Result<()> {
                unsafe { self.$field.SetItem(k, v) }
            }
            fn DeleteItem(&self, k: *const GUID) -> Result<()> {
                unsafe { self.$field.DeleteItem(k) }
            }
            fn DeleteAllItems(&self) -> Result<()> {
                unsafe { self.$field.DeleteAllItems() }
            }
            fn SetUINT32(&self, k: *const GUID, v: u32) -> Result<()> {
                unsafe { self.$field.SetUINT32(k, v) }
            }
            fn SetUINT64(&self, k: *const GUID, v: u64) -> Result<()> {
                unsafe { self.$field.SetUINT64(k, v) }
            }
            fn SetDouble(&self, k: *const GUID, v: f64) -> Result<()> {
                unsafe { self.$field.SetDouble(k, v) }
            }
            fn SetGUID(&self, k: *const GUID, v: *const GUID) -> Result<()> {
                unsafe { self.$field.SetGUID(k, v) }
            }
            fn SetString(&self, k: *const GUID, v: &PCWSTR) -> Result<()> {
                unsafe { self.$field.SetString(k, *v) }
            }
            fn SetBlob(&self, k: *const GUID, buf: *const u8, size: u32) -> Result<()> {
                unsafe {
                    (Interface::vtable(&self.$field).SetBlob)(
                        Interface::as_raw(&self.$field),
                        k,
                        buf,
                        size,
                    )
                    .ok()
                }
            }
            fn SetUnknown(&self, k: *const GUID, v: Ref<IUnknown>) -> Result<()> {
                unsafe { self.$field.SetUnknown(k, v.as_ref()) }
            }
            fn LockStore(&self) -> Result<()> {
                unsafe { self.$field.LockStore() }
            }
            fn UnlockStore(&self) -> Result<()> {
                unsafe { self.$field.UnlockStore() }
            }
            fn GetCount(&self) -> Result<u32> {
                unsafe { self.$field.GetCount() }
            }
            fn GetItemByIndex(&self, i: u32, k: *mut GUID, v: *mut PROPVARIANT) -> Result<()> {
                unsafe { self.$field.GetItemByIndex(i, k, Some(v)) }
            }
            fn CopyAllItems(&self, dest: Ref<IMFAttributes>) -> Result<()> {
                unsafe { self.$field.CopyAllItems(dest.ok()?) }
            }
        }
    };
}

/// IKsControl with no property sets: the frame server asks (camera controls, pins) and
/// takes "not found" as "not supported".
macro_rules! no_ks_control {
    ($impl:ty) => {
        impl IKsControl_Impl for $impl {
            fn KsProperty(
                &self,
                _: *const KSIDENTIFIER,
                _: u32,
                _: *mut core::ffi::c_void,
                _: u32,
                _: *mut u32,
            ) -> Result<()> {
                Err(SET_NOT_FOUND.into())
            }
            fn KsMethod(
                &self,
                _: *const KSIDENTIFIER,
                _: u32,
                _: *mut core::ffi::c_void,
                _: u32,
                _: *mut u32,
            ) -> Result<()> {
                Err(SET_NOT_FOUND.into())
            }
            fn KsEvent(
                &self,
                _: *const KSIDENTIFIER,
                _: u32,
                _: *mut core::ffi::c_void,
                _: u32,
                _: *mut u32,
            ) -> Result<()> {
                Err(SET_NOT_FOUND.into())
            }
        }
    };
}

fn attributes() -> Result<IMFAttributes> {
    let mut a = None;
    unsafe { MFCreateAttributes(&mut a, 8)? };
    a.ok_or_else(|| E_FAIL.into())
}

fn now_variant() -> PROPVARIANT {
    PROPVARIANT::from(unsafe { MFGetSystemTime() })
}

// ---------- class factory ----------

#[implement(IClassFactory)]
pub struct ClassFactory;

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        riid: *const GUID,
        ppv: *mut *mut core::ffi::c_void,
    ) -> Result<()> {
        if !outer.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let activator: IUnknown = Activator::create()?.into();
        unsafe { activator.query(riid, ppv).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}

// ---------- activator ----------

#[implement(IMFActivate)]
struct Activator {
    attrs: IMFAttributes,
    source: Mutex<Option<IMFMediaSource>>,
}

impl Activator {
    fn create() -> Result<IMFActivate> {
        let attrs = attributes()?;
        unsafe {
            attrs.SetUINT32(&MF_VIRTUALCAMERA_PROVIDE_ASSOCIATED_CAMERA_SOURCES, 1)?;
            attrs.SetGUID(
                &MFT_TRANSFORM_CLSID_Attribute,
                &GUID::from_u128(shared::CLSID),
            )?;
        }
        Ok(Self {
            attrs,
            source: Mutex::new(None),
        }
        .into())
    }
}

attributes_via!(Activator_Impl, attrs);

impl IMFActivate_Impl for Activator_Impl {
    fn ActivateObject(&self, riid: *const GUID, ppv: *mut *mut core::ffi::c_void) -> Result<()> {
        let mut source = self.source.lock().unwrap();
        if source.is_none() {
            *source = Some(MediaSource::create(&self.attrs)?);
        }
        unsafe { source.as_ref().unwrap().query(riid, ppv).ok() }
    }

    fn ShutdownObject(&self) -> Result<()> {
        Ok(())
    }

    fn DetachObject(&self) -> Result<()> {
        *self.source.lock().unwrap() = None;
        Ok(())
    }
}

// ---------- media source ----------

#[implement(
    IMFMediaSourceEx,
    IMFGetService,
    IKsControl,
    IMFSampleAllocatorControl,
    IMFAttributes
)]
struct MediaSource {
    attrs: IMFAttributes,
    queue: Mutex<Option<IMFMediaEventQueue>>,
    descriptor: Mutex<Option<IMFPresentationDescriptor>>,
    stream: OnceLock<ComObject<MediaStream>>,
}

impl MediaSource {
    fn create(activator_attrs: &IMFAttributes) -> Result<IMFMediaSource> {
        let attrs = attributes()?;
        unsafe {
            activator_attrs.CopyAllItems(&attrs)?;
            // The profiles Windows Hello and the Camera app look for; one stream, up to 30 fps.
            let profiles = MFCreateSensorProfileCollection()?;
            let profile = MFCreateSensorProfile(&KSCAMERAPROFILE_Legacy, 0, PCWSTR::null())?;
            profile.AddProfileFilter(0, w!("((RES==;FRT<=30,1;SUT==))"))?;
            profiles.AddProfile(&profile)?;
            attrs.SetUnknown(&MF_DEVICEMFT_SENSORPROFILE_COLLECTION, &profiles)?;
        }
        let source = ComObject::new(MediaSource {
            attrs,
            queue: Mutex::new(Some(unsafe { MFCreateEventQueue()? })),
            descriptor: Mutex::new(None),
            stream: OnceLock::new(),
        });
        let as_source: IMFMediaSource = source.to_interface::<IMFMediaSourceEx>().cast()?;
        let stream = MediaStream::create(&as_source)?;
        let desc =
            unsafe { MFCreatePresentationDescriptor(Some(&[Some(stream.descriptor.clone())]))? };
        *source.descriptor.lock().unwrap() = Some(desc);
        let _ = source.stream.set(stream);
        Ok(as_source)
    }

    fn queue(&self) -> Result<IMFMediaEventQueue> {
        self.queue
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| MF_E_SHUTDOWN.into())
    }

    fn stream(&self) -> Result<&ComObject<MediaStream>> {
        self.stream.get().ok_or_else(|| MF_E_SHUTDOWN.into())
    }
}

attributes_via!(MediaSource_Impl, attrs);
no_ks_control!(MediaSource_Impl);

impl IMFMediaEventGenerator_Impl for MediaSource_Impl {
    fn GetEvent(&self, flags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS) -> Result<IMFMediaEvent> {
        let queue = self.queue()?;
        unsafe { queue.GetEvent(flags.0) }
    }

    fn BeginGetEvent(&self, callback: Ref<IMFAsyncCallback>, state: Ref<IUnknown>) -> Result<()> {
        unsafe { self.queue()?.BeginGetEvent(callback.ok()?, state.as_ref()) }
    }

    fn EndGetEvent(&self, result: Ref<IMFAsyncResult>) -> Result<IMFMediaEvent> {
        unsafe { self.queue()?.EndGetEvent(result.ok()?) }
    }

    fn QueueEvent(
        &self,
        met: u32,
        ext: *const GUID,
        status: HRESULT,
        value: *const PROPVARIANT,
    ) -> Result<()> {
        unsafe { self.queue()?.QueueEventParamVar(met, ext, status, value) }
    }
}

impl IMFMediaSource_Impl for MediaSource_Impl {
    fn GetCharacteristics(&self) -> Result<u32> {
        Ok(MFMEDIASOURCE_IS_LIVE.0 as u32)
    }

    fn CreatePresentationDescriptor(&self) -> Result<IMFPresentationDescriptor> {
        let desc = self
            .descriptor
            .lock()
            .unwrap()
            .clone()
            .ok_or(Error::from(MF_E_SHUTDOWN))?;
        unsafe { desc.Clone() }
    }

    fn Start(
        &self,
        pd: Ref<IMFPresentationDescriptor>,
        format: *const GUID,
        _pos: *const PROPVARIANT,
    ) -> Result<()> {
        if !format.is_null() && unsafe { *format } != GUID::zeroed() {
            return Err(MF_E_UNSUPPORTED_TIME_FORMAT.into());
        }
        let pd = pd.ok()?;
        let queue = self.queue()?;
        let ours = self
            .descriptor
            .lock()
            .unwrap()
            .clone()
            .ok_or(Error::from(MF_E_SHUTDOWN))?;
        let stream = self.stream()?;
        let mut selected = BOOL::default();
        let mut desc = None;
        unsafe { pd.GetStreamDescriptorByIndex(0, &mut selected, &mut desc)? };
        let desc = desc.ok_or(Error::from(E_INVALIDARG))?;
        if selected.as_bool() {
            unsafe { ours.SelectStream(0)? };
            let was_running = stream.state() == MF_STREAM_STATE_RUNNING;
            let unk: IUnknown = stream.to_interface();
            let met = if was_running {
                MEUpdatedStream
            } else {
                MENewStream
            };
            unsafe { queue.QueueEventParamUnk(met.0 as u32, &GUID::zeroed(), S_OK, &unk)? };
            let media_type = unsafe { desc.GetMediaTypeHandler()?.GetCurrentMediaType()? };
            stream.start(Some(&media_type))?;
        } else {
            unsafe { ours.DeselectStream(0)? };
            stream.stop()?;
        }
        unsafe {
            queue.QueueEventParamVar(
                MESourceStarted.0 as u32,
                &GUID::zeroed(),
                S_OK,
                &now_variant(),
            )
        }
    }

    fn Stop(&self) -> Result<()> {
        let queue = self.queue()?;
        self.stream()?.stop()?;
        if let Some(desc) = self.descriptor.lock().unwrap().as_ref() {
            unsafe { desc.DeselectStream(0)? };
        }
        unsafe {
            queue.QueueEventParamVar(
                MESourceStopped.0 as u32,
                &GUID::zeroed(),
                S_OK,
                &now_variant(),
            )
        }
    }

    fn Pause(&self) -> Result<()> {
        Err(MF_E_INVALID_STATE_TRANSITION.into())
    }

    fn Shutdown(&self) -> Result<()> {
        let queue = self
            .queue
            .lock()
            .unwrap()
            .take()
            .ok_or(Error::from(MF_E_SHUTDOWN))?;
        unsafe {
            let _ = queue.Shutdown();
        }
        if let Some(stream) = self.stream.get() {
            stream.shutdown();
        }
        *self.descriptor.lock().unwrap() = None;
        Ok(())
    }
}

impl IMFMediaSourceEx_Impl for MediaSource_Impl {
    fn GetSourceAttributes(&self) -> Result<IMFAttributes> {
        Ok(self.attrs.clone())
    }

    fn GetStreamAttributes(&self, id: u32) -> Result<IMFAttributes> {
        if id != 0 {
            return Err(E_INVALIDARG.into());
        }
        Ok(self.stream()?.attrs.clone())
    }

    /// GPU samples are not needed for a CPU-made NV12 frame; the allocator stays in system memory.
    fn SetD3DManager(&self, _manager: Ref<IUnknown>) -> Result<()> {
        Ok(())
    }
}

impl IMFGetService_Impl for MediaSource_Impl {
    fn GetService(
        &self,
        _service: *const GUID,
        _riid: *const GUID,
        _ppv: *mut *mut core::ffi::c_void,
    ) -> Result<()> {
        Err(MF_E_UNSUPPORTED_SERVICE.into())
    }
}

impl IMFSampleAllocatorControl_Impl for MediaSource_Impl {
    fn SetDefaultAllocator(&self, id: u32, allocator: Ref<IUnknown>) -> Result<()> {
        if id != 0 {
            return Err(E_INVALIDARG.into());
        }
        let allocator: IMFVideoSampleAllocatorEx = allocator.ok()?.cast()?;
        *self.stream()?.allocator.lock().unwrap() = Some(allocator);
        Ok(())
    }

    fn GetAllocatorUsage(
        &self,
        id: u32,
        input: *mut u32,
        usage: *mut MFSampleAllocatorUsage,
    ) -> Result<()> {
        if id != 0 || input.is_null() || usage.is_null() {
            return Err(E_INVALIDARG.into());
        }
        unsafe {
            *input = id;
            *usage = MFSampleAllocatorUsage_UsesProvidedAllocator;
        }
        Ok(())
    }
}

// ---------- media stream ----------

#[implement(IMFMediaStream2, IKsControl, IMFAttributes)]
struct MediaStream {
    attrs: IMFAttributes,
    descriptor: IMFStreamDescriptor,
    source: Mutex<Option<IMFMediaSource>>,
    queue: Mutex<Option<IMFMediaEventQueue>>,
    allocator: Mutex<Option<IMFVideoSampleAllocatorEx>>,
    state: Mutex<MF_STREAM_STATE>,
    size: (u32, u32),
    /// Sample requests (their tokens) for the delivery thread.
    requests: Mutex<Option<Sender<Send_<Option<IUnknown>>>>>,
}

/// A COM pointer moved to the delivery thread. The frame server's objects are free-threaded.
struct Send_<T>(T);
unsafe impl<T> Send for Send_<T> {}

impl MediaStream {
    fn create(source: &IMFMediaSource) -> Result<ComObject<MediaStream>> {
        // The size Dotlink writes (its camera size setting); fixed for the stream's life.
        let size = FrameReader::open()
            .ok()
            .and_then(|r| r.size())
            .unwrap_or(DEFAULT_SIZE);
        let attrs = attributes()?;
        let media_type = unsafe {
            attrs.SetGUID(&MF_DEVICESTREAM_STREAM_CATEGORY, &PINNAME_VIDEO_CAPTURE)?;
            attrs.SetUINT32(&MF_DEVICESTREAM_STREAM_ID, 0)?;
            attrs.SetUINT32(&MF_DEVICESTREAM_FRAMESERVER_SHARED, 1)?;
            attrs.SetUINT32(
                &MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES,
                MFFrameSourceTypes_Color.0 as u32,
            )?;

            let t = MFCreateMediaType()?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            t.SetUINT64(&MF_MT_FRAME_SIZE, ((size.0 as u64) << 32) | size.1 as u64)?;
            t.SetUINT64(&MF_MT_FRAME_RATE, ((FPS as u64) << 32) | 1)?;
            t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)?;
            t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            t.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
            t.SetUINT32(&MF_MT_DEFAULT_STRIDE, size.0)?;
            t.SetUINT32(&MF_MT_SAMPLE_SIZE, shared::frame_len(size.0, size.1) as u32)?;
            t.SetUINT32(
                &MF_MT_AVG_BITRATE,
                shared::frame_len(size.0, size.1) as u32 * 8 * FPS,
            )?;
            t
        };
        let descriptor = unsafe { MFCreateStreamDescriptor(0, &[Some(media_type.clone())])? };
        unsafe {
            descriptor
                .GetMediaTypeHandler()?
                .SetCurrentMediaType(&media_type)?
        };

        let (tx, rx) = channel();
        let stream = ComObject::new(MediaStream {
            attrs,
            descriptor,
            source: Mutex::new(Some(source.clone())),
            queue: Mutex::new(Some(unsafe { MFCreateEventQueue()? })),
            allocator: Mutex::new(None),
            state: Mutex::new(MF_STREAM_STATE_STOPPED),
            size,
            requests: Mutex::new(Some(tx)),
        });
        // The thread holds the stream until shutdown closes the request channel.
        let worker = Send_(stream.clone());
        std::thread::Builder::new()
            .name("dotlink-camera".into())
            .spawn(move || {
                let worker = worker;
                deliver(&worker.0, rx)
            })
            .map_err(|_| Error::from(E_FAIL))?;
        Ok(stream)
    }

    fn state(&self) -> MF_STREAM_STATE {
        *self.state.lock().unwrap()
    }

    fn queue(&self) -> Result<IMFMediaEventQueue> {
        self.queue
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| MF_E_SHUTDOWN.into())
    }

    fn start(&self, media_type: Option<&IMFMediaType>) -> Result<()> {
        let queue = self.queue()?;
        if let Some(allocator) = self.allocator.lock().unwrap().as_ref() {
            let t = match media_type {
                Some(t) => t.clone(),
                None => unsafe {
                    self.descriptor
                        .GetMediaTypeHandler()?
                        .GetCurrentMediaType()?
                },
            };
            unsafe { allocator.InitializeSampleAllocator(10, &t)? };
        }
        *self.state.lock().unwrap() = MF_STREAM_STATE_RUNNING;
        unsafe {
            queue.QueueEventParamVar(
                MEStreamStarted.0 as u32,
                &GUID::zeroed(),
                S_OK,
                std::ptr::null(),
            )
        }
    }

    fn stop(&self) -> Result<()> {
        let queue = self.queue()?;
        if let Some(allocator) = self.allocator.lock().unwrap().as_ref() {
            unsafe {
                let _ = allocator.UninitializeSampleAllocator();
            }
        }
        *self.state.lock().unwrap() = MF_STREAM_STATE_STOPPED;
        unsafe {
            queue.QueueEventParamVar(
                MEStreamStopped.0 as u32,
                &GUID::zeroed(),
                S_OK,
                std::ptr::null(),
            )
        }
    }

    fn shutdown(&self) {
        // Ends the delivery thread.
        self.requests.lock().unwrap().take();
        if let Some(queue) = self.queue.lock().unwrap().take() {
            unsafe {
                let _ = queue.Shutdown();
            }
        }
        self.source.lock().unwrap().take();
        self.allocator.lock().unwrap().take();
    }

    /// A sample holding `frame` (NV12 at `self.size`), stamped now.
    fn sample(&self, frame: &[u8]) -> Result<IMFSample> {
        let allocator = self.allocator.lock().unwrap().clone();
        let sample = match allocator {
            Some(a) => unsafe { a.AllocateSample()? },
            None => unsafe {
                let s = MFCreateSample()?;
                let b = MFCreateMemoryBuffer(frame.len() as u32)?;
                s.AddBuffer(&b)?;
                s
            },
        };
        let buffer = unsafe { sample.GetBufferByIndex(0)? };
        let (w, h) = (self.size.0 as usize, self.size.1 as usize);
        if let Ok(b2) = buffer.cast::<IMF2DBuffer>() {
            let mut scan0 = std::ptr::null_mut();
            let mut pitch = 0i32;
            unsafe { b2.Lock2D(&mut scan0, &mut pitch)? };
            let pitch = pitch as isize;
            // NV12 rows: h of Y, then h/2 of interleaved UV, each `w` bytes wide.
            for row in 0..h + h / 2 {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        frame.as_ptr().add(row * w),
                        scan0.offset(row as isize * pitch),
                        w,
                    )
                };
            }
            unsafe { b2.Unlock2D()? };
        } else {
            let mut data = std::ptr::null_mut();
            let mut max = 0u32;
            unsafe { buffer.Lock(&mut data, Some(&mut max), None)? };
            let n = frame.len().min(max as usize);
            unsafe {
                std::ptr::copy_nonoverlapping(frame.as_ptr(), data, n);
                buffer.Unlock()?;
                buffer.SetCurrentLength(n as u32)?;
            }
        }
        unsafe {
            sample.SetSampleTime(MFGetSystemTime())?;
            sample.SetSampleDuration(10_000_000 / FPS as i64)?;
        }
        Ok(sample)
    }
}

/// The delivery thread: one sample per request, no faster than 30 fps, with Dotlink's newest
/// frame (or the last one it had, or a dark frame).
fn deliver(stream: &ComObject<MediaStream>, requests: Receiver<Send_<Option<IUnknown>>>) {
    let size = stream.size;
    let mut frame = vec![0u8; shared::frame_len(size.0, size.1)];
    shared::fill_blank(&mut frame, size.0, size.1);
    let mut scratch = frame.clone();
    let mut reader: Option<FrameReader> = None;
    let mut last_open = Instant::now() - Duration::from_secs(10);
    let period = Duration::from_secs(1) / FPS;
    let mut next = Instant::now();
    while let Ok(Send_(token)) = requests.recv() {
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        }
        next = Instant::now().max(next) + period;
        // Dotlink may start (or restart with another size) after the camera is opened.
        if reader.is_none() && last_open.elapsed() > Duration::from_secs(1) {
            last_open = Instant::now();
            reader = FrameReader::open().ok();
        }
        if let Some(r) = &reader {
            if r.read(&mut scratch, size).is_some() {
                std::mem::swap(&mut frame, &mut scratch);
            } else if r.size().is_some_and(|s| s != size) {
                reader = None;
            }
        }
        if stream.state() != MF_STREAM_STATE_RUNNING {
            continue;
        }
        let Ok(queue) = stream.queue() else { break };
        let Ok(sample) = stream.sample(&frame) else {
            continue;
        };
        unsafe {
            if let Some(token) = &token {
                let _ = sample.SetUnknown(&MFSampleExtension_Token, token);
            }
            let _ =
                queue.QueueEventParamUnk(MEMediaSample.0 as u32, &GUID::zeroed(), S_OK, &sample);
        }
    }
}

attributes_via!(MediaStream_Impl, attrs);
no_ks_control!(MediaStream_Impl);

impl IMFMediaEventGenerator_Impl for MediaStream_Impl {
    fn GetEvent(&self, flags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS) -> Result<IMFMediaEvent> {
        let queue = self.queue()?;
        unsafe { queue.GetEvent(flags.0) }
    }

    fn BeginGetEvent(&self, callback: Ref<IMFAsyncCallback>, state: Ref<IUnknown>) -> Result<()> {
        unsafe { self.queue()?.BeginGetEvent(callback.ok()?, state.as_ref()) }
    }

    fn EndGetEvent(&self, result: Ref<IMFAsyncResult>) -> Result<IMFMediaEvent> {
        unsafe { self.queue()?.EndGetEvent(result.ok()?) }
    }

    fn QueueEvent(
        &self,
        met: u32,
        ext: *const GUID,
        status: HRESULT,
        value: *const PROPVARIANT,
    ) -> Result<()> {
        unsafe { self.queue()?.QueueEventParamVar(met, ext, status, value) }
    }
}

impl IMFMediaStream_Impl for MediaStream_Impl {
    fn GetMediaSource(&self) -> Result<IMFMediaSource> {
        self.source
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| MF_E_SHUTDOWN.into())
    }

    fn GetStreamDescriptor(&self) -> Result<IMFStreamDescriptor> {
        Ok(self.descriptor.clone())
    }

    fn RequestSample(&self, token: Ref<IUnknown>) -> Result<()> {
        if self.state() != MF_STREAM_STATE_RUNNING {
            return Err(MF_E_MEDIA_SOURCE_WRONGSTATE.into());
        }
        let requests = self.requests.lock().unwrap();
        let tx = requests.as_ref().ok_or(Error::from(MF_E_SHUTDOWN))?;
        tx.send(Send_(token.as_ref().cloned()))
            .map_err(|_| Error::from(MF_E_SHUTDOWN))
    }
}

impl IMFMediaStream2_Impl for MediaStream_Impl {
    fn SetStreamState(&self, value: MF_STREAM_STATE) -> Result<()> {
        if self.state() == value {
            return Ok(());
        }
        match value {
            MF_STREAM_STATE_RUNNING => self.start(None),
            MF_STREAM_STATE_STOPPED => self.stop(),
            MF_STREAM_STATE_PAUSED if self.state() == MF_STREAM_STATE_RUNNING => {
                *self.state.lock().unwrap() = value;
                Ok(())
            }
            _ => Err(MF_E_INVALID_STATE_TRANSITION.into()),
        }
    }

    fn GetStreamState(&self) -> Result<MF_STREAM_STATE> {
        Ok(self.state())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::FrameWriter;

    fn event(events: &IMFMediaEventGenerator) -> (u32, PROPVARIANT) {
        unsafe {
            let e = events
                .GetEvent(MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS(0))
                .unwrap();
            (e.GetType().unwrap(), e.GetValue().unwrap())
        }
    }

    /// The source in-process, the way the frame server drives it: activate, start, request
    /// samples. The frame Dotlink writes must come out unchanged.
    #[test]
    fn delivers_written_frames() {
        // A file of its own: the real one may be in use by Dotlink and the frame server.
        let file = std::env::temp_dir().join(format!("dotlink-camera-test-{}.bin", std::process::id()));
        unsafe { std::env::set_var("DOTLINK_CAMERA_FILE", &file) };
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL).unwrap() };
        let (w, h) = (64u32, 48u32);
        let mut writer = FrameWriter::create(w, h).unwrap();
        let pattern: Vec<u8> = (0..shared::frame_len(w, h))
            .map(|i| (i % 251) as u8)
            .collect();
        writer.write(|f| f.copy_from_slice(&pattern));

        let activator = Activator::create().unwrap();
        let source: IMFMediaSource = unsafe { activator.ActivateObject().unwrap() };
        let ex: IMFMediaSourceEx = source.cast().unwrap();
        let pd = unsafe { source.CreatePresentationDescriptor().unwrap() };
        unsafe { pd.SelectStream(0).unwrap() };
        unsafe {
            source
                .Start(&pd, &GUID::zeroed(), &PROPVARIANT::default())
                .unwrap()
        };

        let events: IMFMediaEventGenerator = source.cast().unwrap();
        let (met, stream) = event(&events);
        assert_eq!(met, MENewStream.0 as u32);
        let stream: IMFMediaStream = IUnknown::try_from(&stream).unwrap().cast().unwrap();
        assert_eq!(event(&events).0, MESourceStarted.0 as u32);
        let sgen: IMFMediaEventGenerator = stream.cast().unwrap();
        assert_eq!(event(&sgen).0, MEStreamStarted.0 as u32);
        assert!(unsafe { ex.GetStreamAttributes(0) }.is_ok());

        let started = Instant::now();
        for _ in 0..4 {
            unsafe { stream.RequestSample(None).unwrap() };
            let (met, value) = event(&sgen);
            assert_eq!(met, MEMediaSample.0 as u32);
            let sample: IMFSample = IUnknown::try_from(&value).unwrap().cast().unwrap();
            let buffer = unsafe { sample.ConvertToContiguousBuffer().unwrap() };
            let mut data = std::ptr::null_mut();
            let mut len = 0u32;
            unsafe { buffer.Lock(&mut data, None, Some(&mut len)).unwrap() };
            let got = unsafe { std::slice::from_raw_parts(data, len as usize) }.to_vec();
            unsafe { buffer.Unlock().unwrap() };
            assert_eq!(got, pattern);
        }
        // Four samples at 30 fps: paced, not instant.
        assert!(
            started.elapsed() >= Duration::from_millis(90),
            "{:?}",
            started.elapsed()
        );
        unsafe { source.Shutdown().unwrap() };
        let _ = std::fs::remove_file(&file);
    }
}
