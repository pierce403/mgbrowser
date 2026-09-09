//! The embedding host selects input files and font data; Sparkle returns pixels.
use mg_sparkle::{
    document,
    paint::Fonts,
    render::{self, Controls, Viewport},
};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("Usage: render INPUT.html FONT.ttf OUTPUT.png".into());
    }
    let html = std::fs::read_to_string(&args[0]).map_err(|error| error.to_string())?;
    let bytes = std::fs::read(&args[1]).map_err(|error| error.to_string())?;
    let mut fonts = Fonts::from_bytes(bytes)?;
    let document = document::parse(&html, "https://example.test/");
    let frame = render::render(
        &document,
        &mut fonts,
        Viewport {
            width: 800,
            height: 600,
            scroll: 0,
        },
        &Controls::default(),
    );
    frame.canvas.save_png(&args[2])?;
    println!(
        "{} layout boxes; {} interactive regions",
        frame.boxes.len(),
        frame.hits.len()
    );
    Ok(())
}
