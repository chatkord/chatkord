//! A share's start makes a Vulkan instance of its own while the window paints and names its objects through the
//! loader: the loader must survive a driver unloaded meanwhile.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// How long the share's question is asked over and over beside the painting device.
const FOR: Duration = Duration::from_secs(3);

use tier::skipped;

mod lab {
    use super::*;

    #[test]
    fn the_windows_instance_ends_under_the_loaders_lock() {
        let instance = picture::instance(None);
        if picture::adapters(&instance).is_empty() {
            return skipped("no Vulkan adapter");
        }
        let held = media::alone();
        let (ended, told) = mpsc::channel();
        let ending = std::thread::spawn(move || {
            drop(instance);
            let _ = ended.send(());
        });
        assert!(
            told.recv_timeout(Duration::from_millis(300)).is_err(),
            "the instance's end waits while another thread holds the loader"
        );
        drop(held);
        assert!(told.recv_timeout(Duration::from_secs(5)).is_ok(), "and ends once the loader is free");
        ending.join().unwrap();
    }

    #[test]
    fn a_share_starting_while_the_window_makes_objects_does_not_crash_the_loader() {
        let instance = picture::instance(None);
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            return skipped("no Vulkan adapter");
        };
        let (device, _queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        // The window's side: objects made and dropped all the time, each named as wgpu names it.
        let painting = {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("a picture's buffer"),
                        size: 256,
                        usage: wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    drop(buffer);
                }
            })
        };
        // The share's side: the question its start asks, over and over.
        let until = Instant::now() + FOR;
        let mut asked = 0u32;
        while Instant::now() < until {
            let _ = media::importable();
            asked += 1;
        }
        stop.store(true, Ordering::Relaxed);
        painting.join().unwrap();
        assert!(asked > 0);
    }
}
