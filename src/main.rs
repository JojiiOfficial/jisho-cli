use std::{
    io::{stdin, stdout, Write},
    thread::{self, JoinHandle},
};

use argparse::{ArgumentParser, List, Print, Store, StoreTrue};
use colored::*;
use serde_json::Value;

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
            limit: 4,
            query: String::default(),
            kanji: false,
            interactive: false,
        }
    }
}

fn main() -> Result<(), ureq::Error> {
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
                if i >= options.limit {
                    break;
                }

                if print_item(&query, entry).is_some() && i + 2 <= options.limit {
                    println!();
                }
            }
            println!();
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

fn print_item(query: &str, value: &Value) -> Option<()> {
    let japanese = value_to_arr(value.get("japanese")?).get(0)?.to_owned();

    let reading = japanese
        .get("reading")
        .map(|i| value_to_str(i))
        .unwrap_or(query);

    let word = value_to_str(japanese.get("word").unwrap_or(japanese.get("reading")?));

    println!("{}[{}] {}", word, reading, format_result_tags(value));

    // Print senses
    let senses = value.get("senses")?;
    for (i, sense) in value_to_arr(senses).iter().enumerate() {
        let sense_str = format_sense(&sense, i);
        if sense_str.is_empty() {
            continue;
        }

        println!(" {}", sense_str);
    }

    Some(())
}

fn format_sense(value: &Value, index: usize) -> String {
    let english_definitons = value.get("english_definitions");
    let parts_of_speech = value.get("parts_of_speech");
    if english_definitons.is_none() {
        return "".to_owned();
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
                    "Ichidan verb" => "iru/eru verb",
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

        if parts.is_empty() {
            String::new()
        } else {
            format!("[{}]", parts.bright_blue())
        }
    } else {
        String::new()
    };

    let tags = format_sense_tags(value);

    format!(
        "{}. {} {} {}",
        index + 1,
        english_definiton
            .iter()
            .map(|i| value_to_str(i))
            .collect::<Vec<&str>>()
            .join(", "),
        tags,
        parts_of_speech
    )
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

    if options.limit == 0 {
        options.limit = 1;
    }

    options.query = query_vec.join(" ");
    options
}
