//! Synchronize the public status region with tracked feature states and daily logs.
use std::{env, fs, process};

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.iter().any(|a| a != "--check") || args.len() > 1 {
        return Err("usage: site [--check] (run from repository root)".into());
    }
    let features = fs::read_to_string("FEATURES.md")?;
    let mut title = None;
    let mut entries = Vec::new();
    for line in features.lines() {
        if let Some(value) = line.strip_prefix("## F-") {
            if title.is_some() {
                return Err("feature missing Stability".into());
            }
            title = Some(format!("F-{value}"));
        } else if let Some(state) = line.strip_prefix("Stability: ") {
            if !["planned", "in-progress", "stable"].contains(&state) {
                return Err(format!("invalid stability: {state}").into());
            }
            let name = title.take().ok_or("Stability without feature")?;
            entries.push(format!(
                "  <li><span>{}</span><small>{}</small></li>",
                escape(&name),
                state
            ));
        }
    }
    if entries.is_empty() || title.is_some() {
        return Err("incomplete feature specification".into());
    }
    let mut dates = Vec::new();
    for entry in fs::read_dir("memory/logs")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.len() == 13 && name.ends_with(".md") {
            let date = &name[..10];
            if date.bytes().enumerate().all(|(i, b)| {
                if i == 4 || i == 7 {
                    b == b'-'
                } else {
                    b.is_ascii_digit()
                }
            }) {
                dates.push(date.to_owned());
            }
        }
    }
    dates.sort();
    let date = dates.last().ok_or("no dated work logs")?;
    let html = fs::read_to_string("index.html")?;
    let start = "<!-- project-status:start -->";
    let end = "<!-- project-status:end -->";
    if html.matches(start).count() != 1 || html.matches(end).count() != 1 {
        return Err("expected one status marker pair".into());
    }
    let a = html.find(start).unwrap() + start.len();
    let b = html.find(end).unwrap();
    if a > b {
        return Err("reversed status markers".into());
    }
    let generated = format!(
        "\n<p class=\"muted\">Latest work log: <time datetime=\"{date}\">{date}</time>. Status comes from the repository’s feature specification.</p>\n<ul class=\"status-list\">\n{}\n</ul>\n",
        entries.join("\n")
    );
    let updated = format!("{}{}{}", &html[..a], generated, &html[b..]);
    if args.is_empty() {
        if html != updated {
            fs::write("index.html", updated)?;
        }
        println!("Website status synchronized.");
    } else if html != updated {
        return Err("website status is stale; run tmp/site and commit index.html".into());
    } else {
        println!("Website status is current.");
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}
