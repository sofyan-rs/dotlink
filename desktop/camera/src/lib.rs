//! "Dotlink Camera" on Windows 11: a media source COM server (`dotlink_camera.dll`) that the
//! Windows Camera Frame Server loads for the virtual camera Dotlink registers with
//! MFCreateVirtualCamera. Dotlink writes the phone's frames into a shared file; this reads
//! them. Every app that uses cameras (Teams, Zoom, browsers, OBS, the Camera app) sees it.
//!
//! Setup (Dotlink → Setup → Fix, once, as administrator) copies the DLL to
//! `%ProgramFiles%\Dotlink` and registers [shared::CLSID] under HKLM, where the frame server
//! (Local Service) looks.

// COM exports trip LNK4104 ("should be PRIVATE"), which only matters for import libraries.
#![allow(linker_messages)]

pub mod shared;

#[cfg(windows)]
mod source;

#[cfg(windows)]
mod exports {
    use super::{shared, source};
    use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, E_POINTER, S_FALSE};
    use windows::Win32::System::Com::IClassFactory;
    use windows::core::{GUID, HRESULT, IUnknown, Interface};

    /// COM entry point: the class factory for [shared::CLSID].
    ///
    /// # Safety
    /// Called by COM with valid pointers.
    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn DllGetClassObject(
        clsid: *const GUID,
        iid: *const GUID,
        ppv: *mut *mut core::ffi::c_void,
    ) -> HRESULT {
        if clsid.is_null() || iid.is_null() || ppv.is_null() {
            return E_POINTER;
        }
        unsafe { *ppv = std::ptr::null_mut() };
        if unsafe { *clsid } != GUID::from_u128(shared::CLSID) {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = source::ClassFactory.into();
        unsafe {
            factory
                .cast::<IUnknown>()
                .map_or_else(|e| e.code(), |u| u.query(iid, ppv))
        }
    }

    /// Stays loaded: the frame server keeps the camera around for as long as Dotlink runs.
    #[unsafe(no_mangle)]
    pub extern "system" fn DllCanUnloadNow() -> HRESULT {
        S_FALSE
    }
}
