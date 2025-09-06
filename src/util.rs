use core::fmt::Write;
use heapless::String as HString;
use heapless::Vec as HVec;

/// Utility functions for formatting text and time
/// Formats a quote into displayable lines with configurable parameters
///
/// # Arguments
/// * `quote` - The quote text to format
/// * `max_chars_per_line` - Maximum characters per line (default: 20)
/// * `max_lines` - Maximum number of lines to generate (default: 8)
pub fn format_quote_lines(
    quote: &str,
    max_chars_per_line: usize,
    max_lines: usize,
) -> HVec<HString<64>, 8> {
    let words: HVec<&str, 64> = quote.split_whitespace().collect();
    let mut lines: HVec<HString<64>, 8> = HVec::new();
    let mut current_line = HString::new();

    for word in words {
        // Handle words that are too long to fit on a single line
        if word.len() > max_chars_per_line {
            if !current_line.is_empty() && lines.push(current_line.clone()).is_err() {
                break; // No more lines available
            }
            current_line = HString::new();

            // Break the long word into multiple lines instead of truncating
            let mut remaining_word = word;
            while !remaining_word.is_empty() {
                if lines.len() >= max_lines {
                    break;
                }
                let mut end_idx = core::cmp::min(remaining_word.len(), max_chars_per_line);
                while !remaining_word.is_char_boundary(end_idx) && end_idx > 0 {
                    end_idx -= 1;
                }

                let (chunk, rest) = remaining_word.split_at(end_idx);
                if lines.push(HString::from(chunk)).is_err() {
                    break;
                }
                remaining_word = rest;
            }
            continue;
        }

        // Account for space character when checking length
        let space_needed = if current_line.is_empty() { 0 } else { 1 };
        if current_line.len() + space_needed + word.len() <= max_chars_per_line {
            // Add word to current line
            if !current_line.is_empty() {
                // This is safe because HString capacity (64) > max_chars_per_line (e.g., 20)
                let _ = current_line.push(' ');
            }
            // This is also safe due to the length check on the parent if-statement.
            let _ = current_line.push_str(word);
        } else {
            if !current_line.is_empty() && lines.push(current_line.clone()).is_err() {
                // If we can't add more lines, break
                break;
            }
            current_line = HString::from(word);
        }

        // Check if we've reached the maximum number of lines
        if lines.len() >= max_lines {
            break;
        }
    }
    if !current_line.is_empty() && lines.len() < max_lines {
        let _ = lines.push(current_line);
    }

    lines
}

/// Formats elapsed seconds into a time string with configurable format
///
/// # Arguments
/// * `total_seconds` - Total seconds to format
/// * `show_hours` - Whether to include hours in the format (true: HH:MM:SS, false: MM:SS)
pub fn format_time(total_seconds: u32, show_hours: bool) -> HString<16> {
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    let mut time_str = HString::new();

    if show_hours {
        // Format as HH:MM:SS, ignoring potential errors if the string is full.
        let _ = write!(&mut time_str, "{:02}:", hours);
    }

    // Format minutes and seconds
    let _ = write!(&mut time_str, "{:02}:{:02}", minutes, seconds);

    time_str
}
