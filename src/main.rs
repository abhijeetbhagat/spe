extern crate ffmpeg_next as ffmpeg;
extern crate sdl2;

use sdl2::audio::{AudioCallback, AudioQueue, AudioSpecDesired};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::Rect;
use sdl2::render::Texture;
use sdl2::surface::Surface;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use ffmpeg::format::{input, Pixel};
use ffmpeg::frame::Audio;
use ffmpeg::media::Type;
use ffmpeg::software::scaling::{context::Context as Scaler, flag::Flags};
use ffmpeg::util::frame::video::Video;
use std::env;
use std::fs::File;
use std::io::prelude::*;

struct SoundCallback {
    rx: Receiver<Audio>,
}

impl AudioCallback for SoundCallback {
    type Channel = i16;

    fn callback(&mut self, data: &mut [Self::Channel]) {
        for byte in data.iter_mut() {
            *byte = (f32::sin(0.010 * 4f32) * 5000f32) as i16;
        }
    }
}

fn main() -> Result<(), ffmpeg::Error> {
    ffmpeg::init().unwrap();

    if let Ok(mut ictx) = input(&env::args().nth(1).expect("Cannot open file.")) {
        let in_stream = ictx
            .streams()
            .best(Type::Video)
            .ok_or_else(|| ffmpeg::Error::StreamNotFound)?;

        let in_aud_stream = ictx.streams().best(Type::Audio).ok_or_else(|| {
            println!("No audio found");
            ffmpeg::Error::StreamNotFound
        })?;

        let mut video_decoder = in_stream.codec().decoder().video()?;
        let mut audio_decoder = in_aud_stream.codec().decoder().audio()?;
        //audio_decoder.open_playback();

        let mut scaler = Scaler::get(
            video_decoder.format(),
            video_decoder.width(),
            video_decoder.height(),
            Pixel::YUV420P,
            video_decoder.width(),
            video_decoder.height(),
            Flags::BILINEAR,
        )?;

        let sdl_context = sdl2::init().unwrap();
        let video_subsystem = sdl_context.video().unwrap();
        //video_subsystem.gl_set_swap_interval(1);

        let audio_subsystem = sdl_context.audio().unwrap();
        let specs = AudioSpecDesired {
            channels: None,
            freq: None,
            samples: None,
        };

        let audio_device: AudioQueue<i16> = audio_subsystem
            .open_queue(None, &specs /*, |spec| SoundCallback {}*/)
            .unwrap();

        let mut x = 0f32;
        for i in 0..44100 * 3 {
            x += 0.010f32;
            audio_device.queue(&[(f32::sin(x * 4f32) * 5000f32) as i16][..]);
        }
        audio_device.resume();
        ::std::thread::sleep(Duration::from_secs(3));

        let window = video_subsystem
            .window("spe", video_decoder.width(), video_decoder.height())
            .position_centered()
            .build()
            .unwrap();

        let mut canvas = window
            .into_canvas()
            .accelerated()
            .present_vsync()
            .target_texture()
            .build()
            .unwrap();

        let texture_creator = canvas.texture_creator();
        let mut texture = texture_creator
            .create_texture_streaming(
                PixelFormatEnum::YV12,
                video_decoder.width(),
                video_decoder.height(),
            )
            .unwrap();

        let mut event_pump = sdl_context.event_pump().unwrap();

        for (i, (stream, packet)) in ictx.packets().enumerate() {
            if stream.codec().codec().unwrap().is_video() {
                let mut frame = Video::empty();
                match video_decoder.decode(&packet, &mut frame) {
                    Ok(_) => {
                        let mut yuv_frame = Video::empty();
                        scaler.run(&frame, &mut yuv_frame)?;

                        println!("fps {}", f64::from(stream.rate()));
                        let sleep = 1f64 / f64::from(stream.rate());
                        ::std::thread::sleep(Duration::from_secs_f64(sleep));

                        let rect = Rect::new(0, 0, yuv_frame.width(), yuv_frame.height());
                        println!("rendering frame {}", i);
                        let _ = texture.update_yuv(
                            rect,
                            yuv_frame.data(0),
                            yuv_frame.stride(0),
                            yuv_frame.data(1),
                            yuv_frame.stride(1),
                            yuv_frame.data(2),
                            yuv_frame.stride(2),
                        );

                        canvas.clear();
                        let _ = canvas.copy(&texture, None, None); //copy texture on our canvas
                        canvas.present();
                    }
                    _ => {
                        println!("Error occurred while decoding packet.");
                    }
                }
            } else {
                println!("audio decoding isn't implemented yet");
                let mut frame = Audio::empty();
                match audio_decoder.decode(&packet, &mut frame) {
                    Ok(_) => {} //tx.send(frame),
                    _ => println!("Error occurred while decoding audio"),
                }
            }
        }
    }

    Ok(())
}

fn save_file(frame: &Video, index: usize) -> std::result::Result<(), std::io::Error> {
    let mut file = File::create(format!("frame{}.ppm", index))?;
    file.write_all(format!("P6\n{} {}\n255\n", frame.width(), frame.height()).as_bytes())?;
    file.write_all(frame.data(0))?;
    Ok(())
}
