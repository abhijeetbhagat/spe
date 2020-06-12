extern crate ffmpeg_next as ffmpeg;
extern crate sdl2;

use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::Rect;
use sdl2::render::Texture;
use sdl2::surface::Surface;
use std::time::Duration;

use ffmpeg::format::{input, Pixel};
use ffmpeg::media::Type;
use ffmpeg::software::scaling::{context::Context as Scaler, flag::Flags};
use ffmpeg::util::frame::video::Video;
use std::env;
use std::fs::File;
use std::io::prelude::*;

fn main() -> Result<(), ffmpeg::Error> {
    ffmpeg::init().unwrap();

    if let Ok(mut ictx) = input(&env::args().nth(1).expect("Cannot open file.")) {
        let in_stream = ictx
            .streams()
            .best(Type::Video)
            .ok_or_else(|| ffmpeg::Error::StreamNotFound)?;

        let mut decoder = in_stream.codec().decoder().video()?;

        let mut scaler = Scaler::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::YUV420P,
            decoder.width(),
            decoder.height(),
            Flags::BILINEAR,
        )?;

        let sdl_context = sdl2::init().unwrap();
        let video_subsystem = sdl_context.video().unwrap();
        video_subsystem.gl_set_swap_interval(1);

        let window = video_subsystem
            .window("spe", decoder.width(), decoder.height())
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
            .create_texture_streaming(PixelFormatEnum::YV12, decoder.width(), decoder.height())
            .unwrap();

        let mut event_pump = sdl_context.event_pump().unwrap();
        let mut i = 0;

        for (i, (_, p)) in ictx.packets().enumerate() {
            let mut frame = Video::empty();
            match decoder.decode(&p, &mut frame) {
                Ok(_) => {
                    let mut yuv_frame = Video::empty();
                    scaler.run(&frame, &mut yuv_frame)?;

                    ::std::thread::sleep(Duration::from_millis(33));
                    let rect = Rect::new(0, 0, yuv_frame.width(), yuv_frame.height());
                    println!("rendering frame {}", i);
                    texture.update_yuv(
                        rect,
                        yuv_frame.data(0),
                        yuv_frame.stride(0),
                        yuv_frame.data(1),
                        yuv_frame.stride(1),
                        yuv_frame.data(2),
                        yuv_frame.stride(2),
                    );

                    canvas.clear();
                    canvas.copy(&texture, None, None); //copy texture on our canvas
                    canvas.present();
                }
                _ => {
                    println!("Error occurred while decoding packet.");
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
