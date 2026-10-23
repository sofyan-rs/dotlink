//! Opens "Dotlink Camera" the way apps do (Media Foundation, through the frame server) and
//! reads frames. Needs the camera installed (Setup) and Dotlink running, so it's manual:
//! `cargo test -p dotlink-camera --test capture -- --ignored --nocapture`.
//! Set DOTLINK_CAPTURE_DUMP=1 to keep the last frame's Y plane in %TEMP%\dl-y.raw.
#![cfg(windows)]
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::*;
use windows::core::*;

#[test]
#[ignore = "needs Dotlink running with the camera installed"]
fn capture_frames() {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
        MFStartup(MF_VERSION, MFSTARTUP_FULL).unwrap();
        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 1).unwrap();
        let attrs = attrs.unwrap();
        attrs
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .unwrap();
        let mut list = std::ptr::null_mut();
        let mut count = 0u32;
        MFEnumDeviceSources(&attrs, &mut list, &mut count).unwrap();
        let devices = std::slice::from_raw_parts(list, count as usize);
        let mut found = None;
        for d in devices.iter().flatten() {
            let mut name = PWSTR::null();
            let mut len = 0;
            d.GetAllocatedString(&MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, &mut name, &mut len)
                .unwrap();
            let n = name.to_string().unwrap();
            println!("camera: {n}");
            if n.starts_with("Dotlink Camera") {
                found = Some(d.clone());
            }
        }
        let source: IMFMediaSource = found
            .expect("Dotlink Camera not listed")
            .ActivateObject()
            .unwrap();
        let reader = MFCreateSourceReaderFromMediaSource(&source, None).unwrap();
        let t = reader
            .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)
            .unwrap();
        let size = t.GetUINT64(&MF_MT_FRAME_SIZE).unwrap();
        println!(
            "type: {:?} {}x{}",
            t.GetGUID(&MF_MT_SUBTYPE).unwrap(),
            size >> 32,
            size & 0xffff_ffff
        );
        let start = std::time::Instant::now();
        let mut got = 0;
        while got < 10 {
            let (mut idx, mut flags, mut ts) = (0u32, 0u32, 0i64);
            let mut sample = None;
            reader
                .ReadSample(
                    MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                    0,
                    Some(&mut idx),
                    Some(&mut flags),
                    Some(&mut ts),
                    Some(&mut sample),
                )
                .unwrap();
            if let Some(s) = sample {
                let b = s.ConvertToContiguousBuffer().unwrap();
                let mut p = std::ptr::null_mut();
                let mut l = 0u32;
                b.Lock(&mut p, None, Some(&mut l)).unwrap();
                let (w, h) = ((size >> 32) as usize, (size & 0xffff_ffff) as usize);
                let ys = std::slice::from_raw_parts(p, w * h);
                let avg = ys.iter().map(|&v| v as u64).sum::<u64>() / ys.len() as u64;
                let distinct = {
                    let mut seen = [false; 256];
                    ys.iter().step_by(97).for_each(|&v| seen[v as usize] = true);
                    seen.iter().filter(|&&x| x).count()
                };
                if got == 9 && std::env::var_os("DOTLINK_CAPTURE_DUMP").is_some() {
                    std::fs::write(std::env::temp_dir().join("dl-y.raw"), ys).unwrap();
                }
                b.Unlock().unwrap();
                if got % 3 == 0 {
                    println!("sample {got}: {l} bytes, avg Y={avg}, distinct Y values={distinct}");
                }
                got += 1;
            }
        }
        println!("10 frames in {:?}", start.elapsed());
        source.Shutdown().unwrap();
    }
}
