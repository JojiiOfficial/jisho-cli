use std::{
    io::{stdin, stdout, Write},
    process::{Command, Stdio},
    thread::{self, JoinHandle},
    env,
};

use argparse::{ArgumentParser, List, Print, Store, StoreTrue};
use colored::*;
use serde_json::Value;
use atty::Stream;

macro_rules! JISHO_URL {
    () => {
        "https://jisho.org/api/v1/search/words?keyword={}"
    };
}

#[derive(Debug, Clone)]
struct Options {
    limit: usize,
    query: String,
    kanji: bool, // Sadly not (yet) supported by jisho.org's API
    interactive: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            limit: 0,
            query: String::default(),
            kanji: false,
            interactive: false,
        }
    }
}

fn main() -> Result<(), ureq::Error> {
    let term_size;

    if atty::is(Stream::Stdout) {
        term_size = terminal_size().unwrap_or(0);
    } else {
        term_size = 0;
    }

    let options = parse_args();

    let mut query = {
        if options.interactive {
            let mut o = String::new();
            while o.trim().is_empty() {
                print!("=> ");
                stdout().flush().unwrap();
                stdin().read_line(&mut o).expect("Can't read from stdin");
            }
            o
        } else {
            options.query.clone()
        }
    };

    loop {
        let mut lines_output = 0;
        let mut output = String::new();

        if options.kanji {
            // Open kanji page here
            let threads = query
                .chars()
                .into_iter()
                .map(|kanji| {
                    let kanji = kanji.clone();
                    thread::spawn(move || {
                        webbrowser::open(&format!("https://jisho.org/search/{}%23kanji", kanji))
                            .expect("Couldn't open browser");
                    })
                })
                .collect::<Vec<JoinHandle<()>>>();

            for thread in threads {
                thread.join().unwrap();
            }
        } else {
            // Do API request
            let body: Value = ureq::get(&format!(JISHO_URL!(), query))
                .call()?
                .into_json()?;

            // Try to get the data json-object
            let body = value_to_arr({
                let body = body.get("data");

                if body.is_none() {
                    eprintln!("Error! Invalid response");
                    return Ok(());
                }

                body.unwrap()
            });

            if options.interactive {
                println!();
            }

            // Iterate over meanings and print them
            for (i, entry) in body.iter().enumerate() {
                if i >= options.limit && options.limit != 0 {
                    break;
                }
                match print_item(&query, entry, &mut output) {
                    Some(r) => lines_output += r,
                    None => continue,
                }

                output.push('\n');
                lines_output += 1;
            }
            output.pop();
            if lines_output > 0 {
                lines_output -= 1;
            }

        }

        if lines_output >= term_size - 1 && term_size != 0 {
            // Output is a different process that is not a tty (i.e. less), but we want to keep colour
            env::set_var("CLICOLOR_FORCE", "1");
            pipe_to_less(output);
        } else {
            print!("{}", output);
        }

        if !options.interactive {
            break;
        }

        query.clear();
        while query.trim().is_empty() {
            print!("=> ");
            stdout().flush().unwrap();
            stdin()
                .read_line(&mut query)
                .expect("Can't read from stdin");
        }
    }

    Ok(())
}

fn print_item(query: &str, value: &Value, output: &mut String) -> Option<usize> {
    let japanese = value_to_arr(value.get("japanese")?);
    let main_form = japanese.get(0)?;
    let mut num_of_lines = 0;

    *output += &format!("{} {}\n", format_form(query, main_form)?, format_result_tags(value));

    // Print senses
    let senses = value_to_arr(value.get("senses")?);
    let mut prev_parts_of_speech = String::new();

    for (i, sense) in senses.iter().enumerate() {
        let (sense_str, bump) = format_sense(&sense, i, &mut prev_parts_of_speech);
        if sense_str.is_empty() {
            continue;
        }
        // This bump is to keep count of lines that may or may not be printed (like noun, adverb)
        if bump {
            num_of_lines += 1;
        }

        *output += &format!("    {}\n", sense_str);
    }

    // Print alternative readings and kanji usage
    match japanese.get(1) {
        Some (form) => {
            num_of_lines += 2;

            *output += &format!("    {}", "Other forms\n".bright_blue());

            *output += &format!("    {}", format_form(query, form)?);

            for form in japanese.get(2).iter() {
                *output += &format!(", {}", format_form(query, form)?);
            }
            output.push('\n');
        }
        None => {}
    }

    num_of_lines += senses.iter().count() + 1;
    Some(num_of_lines)
}

fn format_form(query: &str, form: &Value) -> Option<String> {
    let reading = form
        .get("reading")
        .map(|i| value_to_str(i))
        .unwrap_or(query);

    let word = value_to_str(form.get("word").unwrap_or(form.get("reading")?));

    Some(format!("{}[{}]", word, reading))
}

fn format_sense(value: &Value, index: usize, prev_parts_of_speech: &mut String) -> (String, bool) {
    let english_definitons = value.get("english_definitions");
    let parts_of_speech = value.get("parts_of_speech");
    if english_definitons.is_none() {
        return ("".to_owned(), false);
    }

    let english_definiton = value_to_arr(english_definitons.unwrap());

    let parts_of_speech = if let Some(parts_of_speech) = parts_of_speech {
        let parts = value_to_arr(parts_of_speech)
            .to_owned()
            .iter()
            .map(|i| {
                let s = value_to_str(i);
                match s {
                    "Suru verb - irregular" => "Irregular verb",
                    _ => {
                        if s.contains("Godan verb") {
                            "Godan verb"
                        } else {
                            s
                        }
                    }
                }
            })
            .collect::<Vec<&str>>()
            .join(", ");

        // Do not repeat a meaning's part of speech if it is the same as the previous meaning
        if !parts.is_empty() && parts != *prev_parts_of_speech {
            *prev_parts_of_speech = parts.clone();
            format!("{}\n    ", parts.bright_blue())
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let bump = if parts_of_speech.is_empty() {
        false
    } else {
        true
    };

    let index_str = format!("{}.",(index + 1));
    let tags = format_sense_tags(value);

    (format!(
        "{}{} {} {}",
        parts_of_speech,
        index_str.bright_black(),
        english_definiton
            .iter()
            .map(|i| value_to_str(i))
            .collect::<Vec<&str>>()
            .join(", "),
        tags,
    ), bump)
}

/// Format tags from a whole meaning
fn format_result_tags(value: &Value) -> String {
    let mut builder = String::new();

    let is_common_val = value.get("is_common");
    if is_common_val.is_some() && value_to_bool(is_common_val.unwrap()) {
        builder.push_str(&"(common) ".bright_green().to_string());
    }

    if let Some(jlpt) = value.get("jlpt") {
        let jlpt = value_to_arr(&jlpt);
        if !jlpt.is_empty() {
            let jlpt = value_to_str(jlpt.get(0).unwrap())
                .replace("jlpt-", "")
                .to_uppercase();
            builder.push_str(&format!("({}) ", jlpt.bright_blue().to_string()));
        }
    }

    builder
}

/// Format tags from a single sense entry
fn format_sense_tags(value: &Value) -> String {
    let mut builder = String::new();

    if let Some(tags) = value.get("tags") {
        let tags = value_to_arr(tags);

        for tag in tags {
            let t = format_sense_tag(value_to_str(tag));
            builder.push_str(t.as_str())
        }
    }

    builder
}

fn format_sense_tag(tag: &str) -> String {
    match tag {
        "Usually written using kana alone" => "(UK)".to_string(),
        s => format!("({})", s),
    }
}

//
// --- Value helper
//

fn value_to_bool(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        _ => unreachable!(),
    }
}

fn value_to_str(value: &Value) -> &str {
    match value {
        Value::String(s) => s,
        _ => unreachable!(),
    }
}

fn value_to_arr(value: &Value) -> &Vec<Value> {
    match value {
        Value::Array(a) => a,
        _ => unreachable!(),
    }
}

fn parse_args() -> Options {
    let mut options = Options::default();
    let mut query_vec: Vec<String> = Vec::new();
    {
        let mut ap = ArgumentParser::new();
        ap.set_description("Use jisho.org from cli");
        ap.add_option(
            &["-V", "--version"],
            Print(env!("CARGO_PKG_VERSION").to_string()),
            "Show version",
        );
        ap.refer(&mut options.limit).add_option(
            &["-n", "--limit"],
            Store,
            "Limit the amount of results",
        );
        ap.refer(&mut query_vec)
            .add_argument("Query", List, "The query to search for");

        ap.refer(&mut options.interactive).add_option(
            &["-i", "--interactive"],
            StoreTrue,
            "Don't exit after running a query",
        );

        /* Uncomment when supported by jisho.org */
        ap.refer(&mut options.kanji).add_option(
            &["--kanji", "-k"],
            StoreTrue,
            "Look up a certain kanji",
        );

        ap.parse_args_or_exit();
    }

    options.query = query_vec.join(" ");
    options
}

fn pipe_to_less(output: String) {

    let command = Command::new("less")
                    .arg("-R")
                    .stdin(Stdio::piped())
                    .spawn();

    match command {
        Ok(mut process) => {
            if let Err(e) = process.stdin.as_ref().unwrap().write_all(output.as_bytes()) {
                panic!("couldn't pipe to less: {}", e);
            }

            // We don't care about the return value, only whether wait failed or not
            if process.wait().is_err() {
                panic!("wait() was called on non-existent child process\
                 - this should not be possible");
            }
        }

        // less not found in PATH; print normally
        Err(_e) => print!("{}", output)
    };
}

#[cfg(unix)]
fn terminal_size() -> Result<usize, i16> {
    use libc::{c_ushort, ioctl, STDOUT_FILENO, TIOCGWINSZ};

    unsafe {
        let mut size: c_ushort = 0;
        if ioctl(STDOUT_FILENO, TIOCGWINSZ.into(), &mut size as *mut _) != 0 {
            Err(-1)
        } else {
            Ok(size as usize)
        }
    }
}

#[cfg(windows)]
fn terminal_size() -> Result<usize, i16> {
    use windows_sys::Win32::System::Console::*;

    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE) as windows_sys::Win32::Foundation::HANDLE;

        // Unlike the linux function, rust will complain if only part of the struct is sent
        let mut window = CONSOLE_SCREEN_BUFFER_INFO {
            dwSize: COORD { X: 0, Y: 0},
            dwCursorPosition: COORD { X: 0, Y: 0},
            wAttributes: 0,
            dwMaximumWindowSize: COORD {X: 0, Y: 0},
            srWindow: SMALL_RECT {
                Top: 0,
                Bottom: 0,
                Left: 0,
                Right: 0
            }
        };
        if GetConsoleScreenBufferInfo(handle, &mut window) == 0 {
            Err(0)
        } else {
            Ok((window.srWindow.Bottom - window.srWindow.Top) as usize)
        }
    }
}
