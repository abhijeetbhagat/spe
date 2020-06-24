extern crate ffmpeg_next as ffmpeg;
extern crate num_rational;
extern crate sdl2;

use sdl2::audio::{AudioCallback, AudioSpec, AudioSpecDesired};
use sdl2::pixels::PixelFormatEnum;
use sdl2::rect::Rect;
use sdl2::render::Texture;
use sdl2::surface::Surface;
use std::time;

use ffmpeg::format::{input, Pixel};
use ffmpeg::frame::Audio;
use ffmpeg::media::Type;
use ffmpeg::software::scaling::{context::Context as Scaler, flag::Flags};
use ffmpeg::util::format::sample::{Sample, Type as AudioType};
use ffmpeg::util::frame::video::Video;
//use ffmpeg::util::rational::Rational;
use num_rational::Rational64;
use std::{
    cmp,
    collections::VecDeque,
    env,
    sync::mpsc::{channel, Receiver},
};

struct SoundCallback {
    samples: Vec<f32>,
    spec: AudioSpec,
    pos: usize,
    frames: VecDeque<Audio>,
    rx: Receiver<Audio>,
    frame: Option<Audio>,
}

impl AudioCallback for SoundCallback {
    type Channel = i16;

    fn callback(&mut self, out: &mut [Self::Channel]) {
        let mut out_len = out.len();
        let mut pos = 0;
        while out_len > 0 {
            if self.frame.is_none() {
                match self.rx.recv() {
                    Ok(frame) => {
                        self.frame = Some(frame);
                        self.pos = 0;
                    }
                    _ => {
                        for value in out.iter_mut() {
                            *value = 0
                        }
                        return;
                    }
                }
            }

            if let Some(frame) = self.frame.as_ref() {
                let data = frame.plane::<i16>(0);
                let samples = frame.samples();
                let in_len = samples - self.pos;
                let len = cmp::min(out_len, in_len);
                println!(
                    "copying data - len: {}, out len: {}, in len: {}, pos: {}, data len: {}",
                    len,
                    out_len,
                    in_len,
                    self.pos,
                    data.len()
                );
                out[pos..pos + len].copy_from_slice(&data[self.pos..self.pos + len]);
                self.pos += len;
                pos += len;
                out_len -= len;
                if in_len == len {
                    self.frame = None;
                }
            }
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

        let audio_subsystem = sdl_context.audio().unwrap();
        let spec = AudioSpecDesired {
            channels: None,
            freq: None,
            samples: None,
        };

        let (tx, rx) = channel();

        let mut audio_device = audio_subsystem
            .open_playback(None, &spec, |spec| SoundCallback {
                samples: Vec::new(),
                spec,
                pos: 0,
                frames: VecDeque::new(),
                frame: None,
                rx,
            })
            .unwrap(); //open_queue(None, &specs).unwrap();
        audio_device.resume();

        match video_subsystem.gl_set_swap_interval(1) {
            Ok(_) => {}
            _ => println!("error occurred during setting of swap interval"),
        }

        let mut timer = sdl_context.timer().unwrap();

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

        let mut prev_pts = None;
        let mut now = std::time::Instant::now();
        for (i, (stream, packet)) in ictx.packets().enumerate() {
            match stream.codec().codec() {
                Some(codec) if codec.is_video() => {
                    let mut frame = Video::empty();
                    match video_decoder.decode(&packet, &mut frame) {
                        Ok(_) => {
                            let mut yuv_frame = Video::empty();
                            scaler.run(&frame, &mut yuv_frame)?;

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
                            let _ = canvas.copy(&texture, None, None); //copy texture to our canvas
                            canvas.present();

                            let pts = (Rational64::from(packet.pts().unwrap() * 1000000000)
                                * Rational64::new(
                                    stream.time_base().numerator() as i64,
                                    stream.time_base().denominator() as i64,
                                ))
                            .to_integer();
                            //let pts = p.pts().unwrap();
                            println!("pts - {}, timebase - {}", pts, stream.time_base());
                            if let Some(prev) = prev_pts {
                                let elapsed = now.elapsed();
                                if pts > prev {
                                    let sleep = time::Duration::new(0, (pts - prev) as u32);
                                    println!("sleep - {:?}, elapsed - {:?}", sleep, elapsed);
                                    if elapsed < sleep {
                                        println!("sleeping ... ");
                                        std::thread::sleep(sleep - elapsed);
                                    }
                                }
                            }

                            now = time::Instant::now();
                            println!("now - {:?}", now.elapsed());
                            prev_pts = Some(pts);
                        }
                        _ => {
                            println!("Error occurred while decoding packet.");
                        }
                    }
                }
                Some(codec) if codec.is_audio() => {
                    let mut frame = Audio::empty();
                    frame.set_format(Sample::I16(AudioType::Planar));
                    match audio_decoder.decode(&packet, &mut frame) {
                        Ok(_) => {
                            tx.send(frame);
                        }
                        _ => print!("Error decoding audio packet"),
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
