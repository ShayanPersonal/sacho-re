import time
import cv2
import rtmidi
import os
from threading import Thread, Lock
from queue import Queue
from datetime import datetime
import tkinter as tk
from tkinter import messagebox
from collections import deque
import av
import io
import numpy as np
from fractions import Fraction
import copy
import builtins as _builtins
import ctypes

def set_current_thread_priority(level: str) -> None:
    kernel32 = ctypes.windll.kernel32
    mapping = {
        "time_critical":  15,
        "highest":         2,
        "above_normal":    1,
        "normal":          0,
        "below_normal":   -1,
        "lowest":         -2,
        "idle":           -15,
    }
    pri = mapping.get(level, 0)
    h_thread = kernel32.GetCurrentThread()
    kernel32.SetThreadPriority(h_thread, pri)

# Wrap the built-in print to prepend timestamps with millisecond precision
_ORIGINAL_PRINT = _builtins.print

def print(*args, **kwargs):  # noqa: A001 - intentionally shadow built-in for this module
    sep = kwargs.pop('sep', ' ')
    end = kwargs.pop('end', '\n')
    file = kwargs.pop('file', None)
    flush = kwargs.pop('flush', True)
    timestamp = datetime.now().strftime('%Y-%m-%d %H:%M:%S.%f')[:-3]
    message = sep.join(str(arg) for arg in args)
    _ORIGINAL_PRINT(f"[{timestamp}] {message}", end=end, file=file, flush=flush)

# Function to show error messages in a pop-up window
def show_error_message(error_message):
    root = tk.Tk()
    root.withdraw()  # Hide the main window
    messagebox.showerror("Error", error_message)
    root.destroy()

# MIDI event listener setup
class MidiListener:
    def __init__(self):
        self.device_names = [f"RD-88 {i}" for i in range(4)]
        self.midi_in = rtmidi.MidiIn()
        device_index, found_name = self._find_device_index_any()
        if device_index is None:
            raise RuntimeError(f"Could not find any RD-88 MIDI device (tried: {', '.join(self.device_names)})")
        self.device_name = found_name
        self.midi_in.open_port(device_index)
        self.running = True
        self.last_event_time = None
        self.pedal_pressed = False  # Track current pedal state
        self.note_event_occurred = False  # Simple flag instead of queue

    def _find_device_index_any(self):
        for name in self.device_names:
            for i in range(self.midi_in.get_port_count()):
                port_name = self.midi_in.get_port_name(i)
                if name in port_name:
                    print(f"Found {name} at index {i}")
                    return i, name
        return None, None

    def is_device_connected(self):
        for i in range(self.midi_in.get_port_count()):
            port_name = self.midi_in.get_port_name(i)
            if self.device_name in port_name:
                return True
        return False

    def midi_callback(self, message, _):
        midi_message, _ = message
        
        # Handle sustain pedal messages (control change, controller 64)
        if len(midi_message) >= 3 and midi_message[0] == 176 and midi_message[1] == 64:
            pedal_value = midi_message[2]
            
            if pedal_value < 40:
                # Pedal released (below noise threshold)
                if self.pedal_pressed:
                    print(f"Video: Pedal released (value: {pedal_value})")
                    self.last_event_time = time.time()  # Start countdown when pedal is released
                    self.pedal_pressed = False
                else:
                    print(f"Video: Ignoring noisy pedal (value: {pedal_value})")
                return
            else:
                # Pedal pressed (above noise threshold)
                if not self.pedal_pressed:
                    print(f"Video: Pedal pressed (value: {pedal_value})")
                    self.pedal_pressed = True
                self.note_event_occurred = True
                self.last_event_time = time.time()
                return
        
        if midi_message[0] & 0xF0 == 0x90:  # Note on event
            self.note_event_occurred = True
            self.last_event_time = time.time()

    def check_and_clear_event(self):
        """Check if a note event occurred and clear the flag"""
        if self.note_event_occurred:
            self.note_event_occurred = False
            return True
        return False

    def start(self):
        self.midi_in.set_callback(self.midi_callback)

    def stop(self):
        self.midi_in.close_port()
        self.running = False

# Video recorder setup with PyAV encoding
class VideoRecorder:
    def __init__(self, video_source=0):
        self.video_source = video_source
        self.recording = False
        self.capture_running = True
        self.lock = Lock()
        self.encode_lock = Lock()
        
        # Track active recordings to handle overlaps
        self.active_recordings = {}  # recording_id -> {'container': container, 'stream': stream, 'start_time': datetime, 'buffer': BytesIO, 'frame_count': int}
        self.next_recording_id = 0
        self.current_recording_id = None
        
        # Find USB3 Video device by trying different backends and checking device names
        camera_found = False
        available_cameras = []
        print("Scanning for video devices...")

        # Try ID 700 first
        cap = cv2.VideoCapture(700)
        if cap.isOpened():
            print("Found camera at index 700")
            camera_found = True
            self.video_source = 700
            cap.release()
        else:
            print("Camera at index 700 not found")

            # Try different camera indices and backends
            for i in range(1000):  # Check first 1000 camera indices
                cap = cv2.VideoCapture(i)
                if cap.isOpened():
                    # Try to get a frame to verify it's working
                    ret, frame = cap.read()
                    if ret:
                        # Get backend name for debugging
                        backend_name = cap.getBackendName()
                        resolution = f"{frame.shape[1]}x{frame.shape[0]}"
                        print(f"Camera {i}: Backend={backend_name}, Resolution={resolution}")
                        available_cameras.append((i, resolution))
                        
                        # Look for higher resolution cameras (USB3 Video typically has higher res than webcam)
                        if frame.shape[1] >= 1280 or frame.shape[0] >= 720:  # 1080p or higher
                            self.video_source = i
                            camera_found = True
                            print(f"Found high-resolution camera at index {i} (likely USB3 Video)")
                            cap.release()
                            break
                    cap.release()
                else:
                    pass
        
        if not camera_found:
            print("Warning: Could not find high-resolution USB3 Video camera")
            print("Available cameras:")
            for i, resolution in available_cameras:
                print(f"  Camera {i}: {resolution}")
            
            # Ask user to specify which camera to use
            print("Please specify which camera index to use, or press Enter for default (0):")
            try:
                user_input = input().strip()
                if user_input:
                    self.video_source = int(user_input)
                else:
                    self.video_source = 0
            except:
                self.video_source = 0
        
        self.capture = cv2.VideoCapture(self.video_source)
        # Explicitly set capture properties
        self.capture.set(cv2.CAP_PROP_FPS, 30)
        self.capture.set(cv2.CAP_PROP_FRAME_WIDTH, 1920)
        self.capture.set(cv2.CAP_PROP_FRAME_HEIGHT, 1080)
        self.fps = int(self.capture.get(cv2.CAP_PROP_FPS))
        self.frame_size = (
            int(self.capture.get(cv2.CAP_PROP_FRAME_WIDTH)),
            int(self.capture.get(cv2.CAP_PROP_FRAME_HEIGHT))
        )
        
        # Calculate buffer size for preroll_seconds seconds of pre-roll
        self.preroll_seconds = 3
        self.buffer_size = int(self.fps * self.preroll_seconds)
        print(f"Pre-roll buffer: {self.preroll_seconds} seconds ({self.buffer_size} frames)")
        
        # Frame buffer - always limited to preroll_seconds seconds
        self.frame_buffer = deque(maxlen=self.buffer_size)
        
        # Encoding parameters
        self.codec_name = 'hevc_nvenc'
        #self.bit_rate = 8000000  # 8 Mbps
        self.pix_fmt = 'yuv420p10le'
        
        # Encoding queue and thread
        self.encoding_queue = Queue()
        self.encoding_running = True
        
        # Create video recordings directory if it doesn't exist
        video_recordings_dir = "C:/Users/shayan/Desktop/piano/video_recordings"
        if not os.path.exists(video_recordings_dir):
            os.makedirs(video_recordings_dir)
        
        # Start the capture thread
        self.capture_thread = Thread(target=self._capture_frames, daemon=True)
        self.capture_thread.start()
        
        # Start the encoding thread
        self.encoding_thread = Thread(target=self._encoding_worker, daemon=True)
        self.encoding_thread.start()

    def _capture_frames(self):
        """Thread that captures frames and manages the frame buffer"""
        set_current_thread_priority("time_critical")
        while self.capture_running:
            t = time.time()
            ret, frame = self.capture.read()
            if time.time() - t > 0.04:
                print(f"[TIMING] Capture interval spike: {time.time() - t:.3f} seconds")
            frames_to_encode = None  # hand off work outside the capture lock
            if ret and frame is not None:
                with self.lock:
                    # If we're recording and buffer is full, queue the oldest frames for encoding
                    if self.recording and len(self.frame_buffer) >= self.buffer_size:
                        # Swap buffers to minimize time under lock
                        full_buffer = self.frame_buffer
                        self.frame_buffer = deque(maxlen=self.buffer_size)
                        frames_to_encode = list(full_buffer)

                    # Add new frame to buffer
                    self.frame_buffer.append(frame)
                # Queue encoding outside the lock to avoid blocking capture
                if frames_to_encode:
                    self.encoding_queue.put({
                        'type': 'encode_frames',
                        'frames': frames_to_encode,
                        'recording_id': self.current_recording_id
                    })
            else:
                time.sleep(0.01)

    def _encoding_worker(self):
        """Separate thread that handles all encoding operations"""
        while self.encoding_running:
            try:
                # Get encoding task from queue (blocks until available)
                task = self.encoding_queue.get(timeout=1)
                
                if task['type'] == 'encode_frames':
                    self._encode_frames(task['frames'], task['recording_id'])
                elif task['type'] == 'finalize_recording':
                    self._finalize_recording(task['recording_id'])
                
                self.encoding_queue.task_done()
                
            except:
                # Timeout or queue empty, continue
                continue

    def _encode_frames(self, frames, recording_id):
        """Encode frames and mux them directly to the recording's container"""
        if not frames:
            return
            
        print(f"Encoding {len(frames)} frames for recording {recording_id}...")
        t = time.time()
        with self.encode_lock:
            if recording_id not in self.active_recordings:
                print(f"Recording {recording_id} not found")
                return
            
            recording_data = self.active_recordings[recording_id]
            container = recording_data['container']
            stream = recording_data['stream']
            frame_count = recording_data['frame_count']
        
        try:
            # Encode frames with proper timestamps
            for i, frame in enumerate(frames):    
                # Create PyAV frame
                av_frame = av.VideoFrame.from_ndarray(frame, format='bgr24')
                
                # Set frame timestamp (PTS) - continuous across all chunks
                av_frame.pts = frame_count + i
                av_frame.time_base = Fraction(1, self.fps)
                
                # Encode frame and mux packets directly to container
                for packet in stream.encode(av_frame):
                    container.mux(packet)
            
            # Update frame count
            with self.encode_lock:
                if recording_id in self.active_recordings:
                    self.active_recordings[recording_id]['frame_count'] = frame_count + len(frames)
            
            print(f"Encoded and muxed {len(frames)} frames for recording {recording_id}")
            print(f"Time taken: {time.time() - t:.2f} seconds")

        except Exception as e:
            print(f"Error encoding frames: {str(e)}")
            import traceback
            traceback.print_exc()

    def start_recording(self):
        t = time.time()
        """Start recording"""
        if self.recording:
            return
  
        print("Starting recording")

        # Reserve a new recording ID
        recording_id = self.next_recording_id
        self.next_recording_id += 1

        # Prepare container/stream OUTSIDE of the capture lock to avoid blocking frame capture
        recording_start_time = datetime.now()
        output_buffer = io.BytesIO()
        container = av.open(output_buffer, mode='w', format='mp4')

        # Add video stream
        stream = container.add_stream(self.codec_name, rate=self.fps)
        stream.options = {
            "preset": "p7",   # or "p7" for best quality, "p5" for good speed/quality
            "rc": "vbr",
            "cq": "26"        # adjust for your quality target
        }
        stream.width = self.frame_size[0]
        stream.height = self.frame_size[1]
        #stream.pix_fmt = self.pix_fmt
        #stream.bit_rate = self.bit_rate

        # Register the active recording (encoding state) under encode lock
        with self.encode_lock:
            self.active_recordings[recording_id] = {
                'container': container,
                'stream': stream,
                'start_time': recording_start_time,
                'buffer': output_buffer,
                'frame_count': 0
            }

        # Quickly flip recording state and snapshot the preroll buffer under the capture lock
        with self.lock:
            self.recording = True
            self.current_recording_id = recording_id

            if len(self.frame_buffer) > 0:
                frames_to_encode = list(self.frame_buffer)
                self.frame_buffer.clear()
            else:
                frames_to_encode = []

        # Queue the preroll frames for encoding after we've released the capture lock
        if frames_to_encode:
            self.encoding_queue.put({
                'type': 'encode_frames',
                'frames': frames_to_encode,
                'recording_id': recording_id
            })
        print(f"Time taken to start recording: {time.time() - t:.2f} seconds")

    def stop_recording(self):
        """Stop recording and queue finalization"""
        if not self.recording:
            return
            
        with self.lock:
            print("Stopping recording")
            
            recording_id = self.current_recording_id
            
            # Encode any remaining frames in the buffer
            if len(self.frame_buffer) > 0:
                frames_to_encode = list(self.frame_buffer)
                self.encoding_queue.put({
                    'type': 'encode_frames',
                    'frames': frames_to_encode,
                    'recording_id': recording_id
                })
            
            # Queue finalization task
            self.encoding_queue.put({
                'type': 'finalize_recording',
                'recording_id': recording_id
            })
            
            self.recording = False
            self.current_recording_id = None

    def _finalize_recording(self, recording_id):
        """Finalize recording by flushing encoder and writing to file"""
        try:
            with self.encode_lock:
                if recording_id not in self.active_recordings:
                    print(f"Recording {recording_id} not found")
                    return
                
                recording_data = self.active_recordings[recording_id]
                container = recording_data['container']
                stream = recording_data['stream']
                recording_start_time = recording_data['start_time']
                output_buffer = recording_data['buffer']
                frame_count = recording_data['frame_count']
                
                # Remove from active recordings
                del self.active_recordings[recording_id]
            
            # Flush encoder
            for packet in stream.encode():
                container.mux(packet)
            
            # Close container
            container.close()
            
            # Generate filename and write to file
            ts = recording_start_time.strftime("110bpm---%Y-%m-%d---%H-%M-%S")
            video_file = f"C:/Users/shayan/Desktop/piano/video_recordings/{ts}.mp4"
            
            # Write buffer to file
            with open(video_file, 'wb') as f:
                f.write(output_buffer.getvalue())
            
            output_buffer.close()
            
            print(f"Recording saved: {video_file} ({frame_count} frames)")
            
        except Exception as e:
            print(f"Error finalizing recording {recording_id}: {str(e)}")
            import traceback
            traceback.print_exc()

    def release(self):
        """Release all resources"""
        try:
            self.capture_running = False
            self.encoding_running = False
            self.recording = False
            if self.capture is not None:
                self.capture.release()
            print("Resources released")
        except Exception as e:
            print(f"Error releasing resources: {str(e)}")

if __name__ == "__main__":
    midi_listener = MidiListener()
    video_recorder = VideoRecorder(video_source=0)
    print("Set up MIDI and video recorder")

    time.sleep(3)

    last_device_check = time.time()
    device_connected = True

    try:
        midi_listener.start()
        print("Listening for MIDI events...")

        while True:
            # Hotplug check every 2 seconds
            if time.time() - last_device_check > 2:
                if not midi_listener.is_device_connected():
                    if device_connected:
                        print("MIDI device disconnected!")
                        device_connected = False
                        midi_listener.stop()
                else:
                    if not device_connected:
                        print("MIDI device reconnected!")
                        try:
                            midi_listener = MidiListener()
                            midi_listener.start()
                            device_connected = True
                        except Exception as e:
                            print(f"Reconnect failed: {e}")
                last_device_check = time.time()

            if device_connected:
                if midi_listener.check_and_clear_event() and not video_recorder.recording:
                    video_recorder.start_recording()

                # Stop recording after preroll_seconds seconds of no MIDI events, but only if pedal is not pressed
                if video_recorder.recording and midi_listener.last_event_time:
                    if not midi_listener.pedal_pressed and time.time() - midi_listener.last_event_time > 7:
                        video_recorder.stop_recording()
                        while midi_listener.check_and_clear_event():
                            pass

            time.sleep(0.1)

    except KeyboardInterrupt:
        print("Exiting...")

    except Exception as e:
        error_message = f"An error occurred:\n{str(e)}"
        print(error_message)  # Print the error in the console
        show_error_message(error_message)  # Display the error in a popup

    finally:
        midi_listener.stop()
        video_recorder.release() 