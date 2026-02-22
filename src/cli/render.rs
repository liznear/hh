use std::io::{self, Write};

pub fn print_assistant(text: &str) {
    println!("assistant> {}", text);
}

pub fn print_tool_log(name: &str, message: &str) {
    println!("tool:{}> {}", name, message);
}

pub fn print_error(message: &str) {
    eprintln!("error: {}", message);
}

pub fn prompt_user() -> io::Result<String> {
    print!("you> ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

pub fn confirm(prompt: &str) -> io::Result<bool> {
    print!("{} [y/N]: ", prompt);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let normalized = input.trim().to_ascii_lowercase();
    Ok(normalized == "y" || normalized == "yes")
}
