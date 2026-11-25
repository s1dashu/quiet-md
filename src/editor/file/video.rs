//! AVPlayer's decoded frames go straight to GPUI's Metal surfaces. No Quick Look window.
use gpui::{AnyElement, IntoElement, Styled, px, surface};
use objc::runtime::{Object, BOOL, YES};
use objc::{class, msg_send, sel, sel_impl};
use objc2_core_foundation::CFRetained;
use objc2_core_video::{CVPixelBuffer, CVPixelBufferGetHeight, CVPixelBufferGetWidth, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use std::time::{Duration, Instant};

#[repr(C)]
#[derive(Clone, Copy)]
struct Time { value: i64, scale: i32, flags: u32, epoch: i64 }
unsafe impl objc::Encode for Time {
    fn encode() -> objc::Encoding { unsafe { objc::Encoding::from_str("{CMTime=qiIq}") } }
}
impl Time {
    fn seconds(self) -> f64 {
        if self.flags & 1 == 0 || self.flags & 28 != 0 || self.scale <= 0 { return 0.; }
        (self.value as f64 / self.scale as f64).max(0.)
    }
    fn at(t: f64) -> Self { Self { value: (t * 600.).round() as i64, scale: 600, flags: 1, epoch: 0 } }
}

pub struct Player {
    player: *mut Object,
    item: *mut Object,
    output: *mut Object,
    frame: RefCell<Option<CFRetained<CVPixelBuffer>>>,
    refresh_until: Cell<Instant>,
    seeking: Arc<AtomicU64>,
    seek_serial: Cell<u64>,
    seek_target: Cell<f64>,
    seek_frame: Cell<bool>,
    frame_duration: f64,
}

impl Player {
    pub fn open(path: &Path) -> Option<Self> {
        let s = path.to_str()?;
        unsafe {
            let ns: *mut Object = msg_send![class!(NSString), alloc];
            let ns: *mut Object = msg_send![ns, initWithBytes: s.as_ptr() length: s.len() encoding: 4usize];
            if ns.is_null() { return None; }
            let url: *mut Object = msg_send![class!(NSURL), fileURLWithPath: ns];
            let _: () = msg_send![ns, release];
            if url.is_null() { return None; }
            let item: *mut Object = msg_send![class!(AVPlayerItem), alloc];
            let item: *mut Object = msg_send![item, initWithURL: url];
            if item.is_null() { return None; }
            // Respect the track's display transform (including portrait phone recordings).
            let asset: *mut Object = msg_send![item, asset];
            let playable: BOOL = msg_send![asset, isPlayable];
            let tracks: *mut Object = msg_send![asset, tracksWithMediaType: crate::printing::native::ns_string("vide")];
            let count: usize = msg_send![tracks, count];
            if playable != YES || count == 0 {
                let _: () = msg_send![item, release];
                return None;
            }
            let composition: *mut Object = msg_send![class!(AVVideoComposition), videoCompositionWithPropertiesOfAsset: asset];
            if composition.is_null() { let _: () = msg_send![item, release]; return None; }
            let _: () = msg_send![item, setVideoComposition: composition];
            let frame_duration: Time = msg_send![composition, frameDuration];
            let format: *mut Object = msg_send![class!(NSNumber), numberWithUnsignedInt: kCVPixelFormatType_420YpCbCr8BiPlanarFullRange];
            let attrs: *mut Object = msg_send![class!(NSDictionary), dictionaryWithObject: format forKey: crate::printing::native::ns_string("PixelFormatType")];
            let output: *mut Object = msg_send![class!(AVPlayerItemVideoOutput), alloc];
            let output: *mut Object = msg_send![output, initWithPixelBufferAttributes: attrs];
            if output.is_null() { let _: () = msg_send![item, release]; return None; }
            let _: () = msg_send![item, addOutput: output];
            let player: *mut Object = msg_send![class!(AVPlayer), alloc];
            let player: *mut Object = msg_send![player, initWithPlayerItem: item];
            if player.is_null() {
                let _: () = msg_send![output, release];
                let _: () = msg_send![item, release];
                return None;
            }
            Some(Self { player, item, output, frame: RefCell::new(None), refresh_until: Cell::new(Instant::now() + Duration::from_secs(15)), seeking: Arc::new(AtomicU64::new(0)), seek_serial: Cell::new(0), seek_target: Cell::new(0.), seek_frame: Cell::new(false), frame_duration: frame_duration.seconds() })
        }
    }

    pub fn failed(&self) -> bool { unsafe { let status: isize = msg_send![self.item, status]; status == 2 } }
    pub fn needs_tick(&self) -> bool { !self.failed() && (self.playing() || Instant::now() < self.refresh_until.get()) }
    pub fn play(&self) {
        let d = self.duration();
        if d > 0. && self.time() >= d - 0.05 { self.seek(0.); }
        unsafe { let _: () = msg_send![self.player, play]; }
    }
    pub fn pause(&self) { unsafe { let _: () = msg_send![self.player, pause]; } }
    pub fn playing(&self) -> bool { unsafe { let rate: f32 = msg_send![self.player, rate]; rate > 0. } }
    pub fn time(&self) -> f64 { unsafe { let t: Time = msg_send![self.player, currentTime]; t.seconds() } }
    pub fn duration(&self) -> f64 { unsafe { let t: Time = msg_send![self.item, duration]; t.seconds() } }
    pub fn seek_time(&self) -> f64 { if self.seeking.load(Ordering::Acquire) != 0 { self.seek_target.get() } else { self.time() } }
    pub fn seek(&self, t: f64) {
        self.seek_target.set(t);
        self.refresh_until.set(Instant::now() + Duration::from_secs(10));
        self.seek_frame.set(true);
        let serial = self.seek_serial.get().wrapping_add(1).max(1);
        self.seek_serial.set(serial);
        self.seeking.store(serial, Ordering::Release);
        let seeking = self.seeking.clone();
        let completion = block2::RcBlock::new(move |_: objc2::runtime::Bool| {
            let _ = seeking.compare_exchange(serial, 0, Ordering::AcqRel, Ordering::Acquire);
        });
        unsafe {
            let _: () = msg_send![self.player, seekToTime: Time::at(t) toleranceBefore: Time::at(0.) toleranceAfter: Time::at(0.) completionHandler: block2::RcBlock::as_ptr(&completion).cast::<std::ffi::c_void>()];
        }
    }
    pub fn set_volume(&self, v: f32) { unsafe { let _: () = msg_send![self.player, setVolume: v]; } }

    pub fn picture(&self, width: f32, height: Option<f32>) -> Option<AnyElement> {
        unsafe {
            let t: Time = msg_send![self.player, currentTime];
            let new: BOOL = msg_send![self.output, hasNewPixelBufferForItemTime: t];
            if new == YES && self.seeking.load(Ordering::Acquire) == 0 {
                let mut display = Time::at(0.);
                let buffer: *mut CVPixelBuffer = msg_send![self.output, copyPixelBufferForItemTime: t itemTimeForDisplay: &mut display];
                if let Some(buffer) = NonNull::new(buffer) {
                    // The copy method transfers +1 ownership; the surface retains it through painting.
                    let frame = CFRetained::from_raw(buffer);
                    // AVPlayer can finish a seek before its output flushes the
                    // previous frame. Keep polling until the new time is decoded.
                    if !self.seek_frame.get() || (display.seconds() - t.seconds()).abs() <= (self.frame_duration * 1.5).max(0.1) {
                        self.seek_frame.set(false);
                        self.refresh_until.set(Instant::now());
                        self.frame.replace(Some(frame));
                    }
                }
            }
        }
        let frame = self.frame.borrow().clone()?;
        let h = width * CVPixelBufferGetHeight(&frame) as f32 / (CVPixelBufferGetWidth(&frame) as f32).max(1.);
        Some(surface(frame).w(px(width)).h(px(height.unwrap_or(h.clamp(160., 600.)))).into_any_element())
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.pause();
        unsafe {
            let _: () = msg_send![self.item, removeOutput: self.output];
            let _: () = msg_send![self.player, release];
            let _: () = msg_send![self.output, release];
            let _: () = msg_send![self.item, release];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Time;

    #[test]
    fn unavailable_duration_does_not_reach_the_seek_bar() {
        assert_eq!(Time { value: 60, scale: 0, flags: 1, epoch: 0 }.seconds(), 0.);
        for flags in [0, 5, 9, 17] {
            assert_eq!(Time { value: 600, scale: 600, flags, epoch: 0 }.seconds(), 0.);
        }
        assert_eq!(Time::at(1.5).seconds(), 1.5);
        assert_eq!(Time::at(-1.).seconds(), 0.);
    }
}
