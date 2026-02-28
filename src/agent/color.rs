use ratatui::style::Color;

pub fn parse_color(s: &str) -> Option<Color> {
    match s.to_lowercase().as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "lightblack" | "gray" | "grey" => Some(Color::DarkGray),
        "lightred" => Some(Color::LightRed),
        "lightgreen" => Some(Color::LightGreen),
        "lightyellow" => Some(Color::LightYellow),
        "lightblue" => Some(Color::LightBlue),
        "lightmagenta" | "lightpink" | "pink" => Some(Color::LightMagenta),
        "lightcyan" => Some(Color::LightCyan),
        "lightwhite" => Some(Color::White),
        // Try to parse as RGB hex
        hex => {
            let hex = hex.trim_start_matches('#');
            if hex.len() == 6
                && let (Ok(r), Ok(g), Ok(b)) = (
                    u8::from_str_radix(&hex[0..2], 16),
                    u8::from_str_radix(&hex[2..4], 16),
                    u8::from_str_radix(&hex[4..6], 16),
                )
            {
                return Some(Color::Rgb(r, g, b));
            }
            None
        }
    }
}

pub fn default_agent_color() -> Color {
    Color::White
}
