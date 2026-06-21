//! CoreAudio backend for macOS and iOS.
//!
//! Uses `AudioUnit` with a render callback to drive the pull-based audio
//! pipeline. The render callback is invoked on a high-priority audio thread
//! by the system and calls [`AudioCallback::pull`] to fetch PCM samples.
//!

use coreaudio_sys::{
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM,
    kAudioUnitProperty_StreamFormat, kAudioUnitScope_Input, kAudioUnitScope_Output,
    kAudioUnitType_Output, AudioComponentDescription,
    AudioComponentFindNext, AudioComponentInstanceDispose, AudioComponentInstanceNew,
    AudioOutputUnitStart, AudioOutputUnitStop, AudioStreamBasicDescription, AudioUnitGetProperty,
    AudioUnitInitialize, AudioUnitRenderActionFlags, AudioUnitSetProperty, AudioUnitUninitialize,
};

#[cfg(target_os = "macos")]
use coreaudio_sys::kAudioUnitSubType_DefaultOutput as kOutputUnitSubType;
#[cfg(target_os = "ios")]
use coreaudio_sys::kAudioUnitSubType_GenericOutput as kOutputUnitSubType;

use std::ffi::c_void;

use crate::error::AudioError;
use crate::hal::AudioCallback;
use crate::hal::AudioDevice;
use crate::types::AudioFormat;

/// CoreAudio `noErr` status code (OSStatus = i32).
const NO_ERR: i32 = 0;

/// Holds a fat pointer to the callback trait object. Because
/// `AURenderCallbackStruct.inputProcRefCon` is a thin `*mut c_void`,
/// we store an indirection: a `Box<CallbackRef>` whose only field is
/// the fat `*const dyn AudioCallback` pointer.
struct CallbackRef {
    ptr: *const dyn AudioCallback,
}
// Safety: CallbackRef is only accessed from the audio thread via &self.
unsafe impl Send for CallbackRef {}
unsafe impl Sync for CallbackRef {}

/// An audio output device backed by CoreAudio (AudioUnit).
pub struct CoreAudioDevice {
    /// Hardware format detected at open time.
    format: AudioFormat,
    audio_unit: Option<sys::AudioUnit>,
    /// Owns the callback Box and the CallbackRef indirection.
    /// Kept alive for the lifetime of the device.
    _callback_box: Option<Box<dyn AudioCallback>>,
    _callback_ref: Option<Box<CallbackRef>>,
    running: bool,
}

// Safety: CoreAudio handles are safe to send between threads.
unsafe impl Send for CoreAudioDevice {}

// ── Render callback trampoline (C ABI) ─────────────────────────────────

/// The C-callable render callback installed on the AudioUnit.
///
/// Bridges from the CoreAudio callback signature to our Rust
/// `AudioCallback::pull()` method. **Lock-free**: the callback is
/// accessed via `&self` through a raw `*const dyn AudioCallback`.
extern "C" fn render_callback(
    _in_ref_con: *mut c_void,
    _io_action_flags: *mut AudioUnitRenderActionFlags,
    _in_time_stamp: *const sys::AudioTimeStamp,
    _in_bus_number: u32,
    in_number_frames: u32,
    io_data: *mut sys::AudioBufferList,
) -> i32 {
    // Safety: _in_ref_con points to a CallbackRef that outlives the
    // AudioUnit. We access it immutably — zero locks.
    if _in_ref_con.is_null() || io_data.is_null() {
        return NO_ERR;
    }

    let cb_ref: &CallbackRef = unsafe { &*(_in_ref_con as *const CallbackRef) };
    if cb_ref.ptr.is_null() {
        return NO_ERR;
    }
    let cb: &dyn AudioCallback = unsafe { &*cb_ref.ptr };
    let buffers = unsafe { &mut *io_data };
    let frame_count = in_number_frames as usize;

    if buffers.mNumberBuffers == 0 {
        return NO_ERR;
    }
    let buf_ptr = buffers.mBuffers[0].mData as *mut f32;
    if buf_ptr.is_null() {
        return NO_ERR;
    }

    let channel_count = buffers.mBuffers[0].mNumberChannels as usize;
    let sample_count = frame_count * channel_count;
    let output = unsafe { std::slice::from_raw_parts_mut(buf_ptr, sample_count) };

    // Pull samples — zero locks, &self access.
    let frames_written = cb.pull(output);

    // Clamp to the actual frame count to guard against a buggy / malicious
    // callback returning more frames than the buffer can hold (prevents both
    // an out-of-bounds slice panic and a usize overflow below).
    let frames_written = frames_written.min(frame_count);

    // If the callback returned fewer frames, zero-fill the remainder.
    let written_samples = frames_written * channel_count;
    if written_samples < sample_count {
        output[written_samples..].fill(0.0);
    }

    NO_ERR
}

// ── CoreAudioDevice ────────────────────────────────────────────────────

impl CoreAudioDevice {
    /// Create a new CoreAudio output device.
    ///
    /// Queries the hardware's native stream format (sample rate + channels)
    /// from the default output AudioUnit. The sample format is forced to
    /// 32-bit float interleaved — the lowest-latency path through CoreAudio.
    pub fn new() -> Result<Self, AudioError> {
        let desc = AudioComponentDescription {
            componentType: kAudioUnitType_Output,
            componentSubType: kOutputUnitSubType,
            componentManufacturer: 0x6170706c, // 'appl'
            componentFlags: 0,
            componentFlagsMask: 0,
        };

        // Find the default output component.
        let component = unsafe { AudioComponentFindNext(std::ptr::null_mut(), &desc as *const _) };
        if component.is_null() {
            return Err(AudioError::DeviceNotFound);
        }

        // Instantiate the AudioUnit.
        let mut audio_unit: sys::AudioUnit = std::ptr::null_mut();
        let status = unsafe { AudioComponentInstanceNew(component, &mut audio_unit) };
        if status != NO_ERR || audio_unit.is_null() {
            return Err(AudioError::DeviceBusy);
        }

        // Query the hardware's native output stream format to get the
        // optimal sample rate and channel count for lowest latency.
        let mut hw_asbd: AudioStreamBasicDescription = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of::<AudioStreamBasicDescription>() as u32;
        let status = unsafe {
            AudioUnitGetProperty(
                audio_unit,
                kAudioUnitProperty_StreamFormat,
                kAudioUnitScope_Output,
                0, // output bus
                &mut hw_asbd as *mut _ as *mut c_void,
                &mut size,
            )
        };
        // Fall back to sensible defaults if the query fails.
        let (sample_rate, channels) = if status == NO_ERR && hw_asbd.mChannelsPerFrame > 0 {
            (hw_asbd.mSampleRate as u32, hw_asbd.mChannelsPerFrame as u16)
        } else {
            (48000, 2)
        };

        let format = AudioFormat::new(sample_rate, channels);

        // Set the input stream format to 32-bit float at the hardware rate.
        let asbd = AudioStreamBasicDescription {
            mSampleRate: format.sample_rate as f64,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
            mBytesPerPacket: (format.channels as u32) * 4,
            mFramesPerPacket: 1,
            mBytesPerFrame: (format.channels as u32) * 4,
            mChannelsPerFrame: format.channels as u32,
            mBitsPerChannel: 32,
            mReserved: 0,
        };

        let status = unsafe {
            AudioUnitSetProperty(
                audio_unit,
                kAudioUnitProperty_StreamFormat,
                kAudioUnitScope_Input,
                0, // output bus
                &asbd as *const _ as *const c_void,
                std::mem::size_of::<AudioStreamBasicDescription>() as u32,
            )
        };
        if status != NO_ERR {
            unsafe { AudioComponentInstanceDispose(audio_unit) };
            return Err(AudioError::FormatNotSupported);
        }

        Ok(Self {
            format,
            audio_unit: Some(audio_unit),
            _callback_box: None,
            _callback_ref: None,
            running: false,
        })
    }
}

impl AudioDevice for CoreAudioDevice {
    fn format(&self) -> AudioFormat {
        self.format
    }
    fn start(&mut self, callback: Box<dyn AudioCallback>) -> Result<(), AudioError> {
        let au = self.audio_unit.as_ref().ok_or(AudioError::DeviceNotFound)?;

        if self.running {
            return Ok(());
        }

        // Initialize the AudioUnit (must be done once before start).
        let status = unsafe { AudioUnitInitialize(*au) };
        if status != NO_ERR {
            return Err(AudioError::BackendError(format!(
                "AudioUnitInitialize failed: {status}"
            )));
        }

        // Because `AURenderCallbackStruct.inputProcRefCon` is a thin
        // `*mut c_void`, it cannot hold a fat trait-object pointer
        // (data ptr + vtable). We use an indirection: a Box<CallbackRef>
        // that stores the fat pointer. The device owns both the callback
        // Box and the CallbackRef Box.
        //
        // AudioCallback::pull() takes `&self` — no Mutex needed.
        let fat_ptr: *const dyn AudioCallback = Box::into_raw(callback);
        let cb_ref = Box::new(CallbackRef { ptr: fat_ptr });
        let ref_con: *mut c_void = Box::into_raw(cb_ref) as *mut c_void;

        // Set the render callback.
        let cb_struct = sys::AURenderCallbackStruct {
            inputProc: Some(render_callback),
            inputProcRefCon: ref_con,
        };

        let status = unsafe {
            AudioUnitSetProperty(
                *au,
                sys::kAudioUnitProperty_SetRenderCallback,
                kAudioUnitScope_Input,
                0, // output bus
                &cb_struct as *const _ as *const c_void,
                std::mem::size_of::<sys::AURenderCallbackStruct>() as u32,
            )
        };
        if status != NO_ERR {
            // Clean up on failure.
            unsafe {
                let _ = Box::from_raw(ref_con as *mut CallbackRef);
                drop(Box::from_raw(fat_ptr as *mut dyn AudioCallback));
            }
            return Err(AudioError::BackendError(format!(
                "failed to set render callback: {status}"
            )));
        }

        // Start the AudioUnit.
        let status = unsafe { AudioOutputUnitStart(*au) };
        if status != NO_ERR {
            // Clean up on failure.
            unsafe {
                let _ = Box::from_raw(ref_con as *mut CallbackRef);
                drop(Box::from_raw(fat_ptr as *mut dyn AudioCallback));
            }
            return Err(AudioError::BackendError(format!(
                "AudioOutputUnitStart failed: {status}"
            )));
        }

        // Reconstruct the boxes so Drop can reclaim them.
        // Safety: ref_con and fat_ptr were created from Box::into_raw just above.
        self._callback_ref = Some(unsafe { Box::from_raw(ref_con as *mut CallbackRef) });
        self._callback_box = Some(unsafe { Box::from_raw(fat_ptr as *mut dyn AudioCallback) });
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if let Some(au) = self.audio_unit.as_ref() {
            if self.running {
                unsafe {
                    AudioOutputUnitStop(*au);
                    AudioUnitUninitialize(*au);
                }
            }
        }
        self.running = false;

        // Reclaim the callback (order matters: drop callback before CallbackRef).
        self._callback_box = None;
        self._callback_ref = None;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        if let Some(au) = self.audio_unit.as_ref() {
            if self.running {
                unsafe {
                    AudioOutputUnitStop(*au);
                }
            }
        }
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        if let Some(au) = self.audio_unit.as_ref() {
            if self.running {
                let status = unsafe { AudioOutputUnitStart(*au) };
                if status != NO_ERR {
                    return Err(AudioError::BackendError(format!(
                        "AudioOutputUnitStart failed: {status}"
                    )));
                }
            }
        }
        Ok(())
    }
}

impl Drop for CoreAudioDevice {
    fn drop(&mut self) {
        if self.running {
            let _ = self.stop();
        }
        // Ensure callbacks are dropped before the AudioUnit.
        self._callback_box = None;
        self._callback_ref = None;
        if let Some(au) = self.audio_unit.take() {
            unsafe {
                AudioComponentInstanceDispose(au);
            }
        }
    }
}

/// Internal alias for the coreaudio-sys re-exports, keeping things readable.
mod sys {
    pub use coreaudio_sys::*;
}
