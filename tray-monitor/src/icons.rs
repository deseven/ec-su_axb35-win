//! Dynamic icon rendering module
//! Creates 16x16 icons with colored backgrounds and text overlays

use image::{Rgba, RgbaImage};
use tray_icon::Icon;

/// Icon size for system tray
const ICON_SIZE: u32 = 16;

/// Embedded bitmap font data for digits and special characters (5x7 pixels each)
/// Each character is represented as a 7-row array of 5-bit patterns
const FONT_DATA: &[(char, [u8; 7])] = &[
    ('0', [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110]),
    ('1', [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
    ('2', [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111]),
    ('3', [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110]),
    ('4', [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010]),
    ('5', [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110]),
    ('6', [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110]),
    ('7', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000]),
    ('8', [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110]),
    ('9', [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100]),
    ('.', [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100]),
    ('K', [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001]),
    ('°', [0b01100, 0b10010, 0b01100, 0b00000, 0b00000, 0b00000, 0b00000]),
    ('?', [0b01110, 0b10001, 0b00001, 0b00110, 0b00100, 0b00000, 0b00100]),
    ('-', [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000]),
];

/// Get bitmap data for a character
fn get_char_bitmap(c: char) -> Option<[u8; 7]> {
    FONT_DATA.iter().find(|(ch, _)| *ch == c).map(|(_, data)| *data)
}

/// Draw a character onto the image at the specified position
fn draw_char(img: &mut RgbaImage, c: char, x: i32, y: i32, color: Rgba<u8>) {
    if let Some(bitmap) = get_char_bitmap(c) {
        for (row_idx, row) in bitmap.iter().enumerate() {
            for col in 0..5 {
                if (row >> (4 - col)) & 1 == 1 {
                    let px = x + col;
                    let py = y + row_idx as i32;
                    if px >= 0 && px < ICON_SIZE as i32 && py >= 0 && py < ICON_SIZE as i32 {
                        img.put_pixel(px as u32, py as u32, color);
                    }
                }
            }
        }
    }
}

/// Draw text centered on the icon
fn draw_text_centered(img: &mut RgbaImage, text: &str, color: Rgba<u8>) {
    let char_width = 5;
    let char_spacing = 1;
    let text_width: i32 = text.len() as i32 * (char_width + char_spacing) - char_spacing;
    
    let start_x = (ICON_SIZE as i32 - text_width) / 2;
    let start_y = (ICON_SIZE as i32 - 7) / 2; // 7 is char height
    
    for (i, c) in text.chars().enumerate() {
        let x = start_x + i as i32 * (char_width + char_spacing);
        draw_char(img, c, x, start_y, color);
    }
}

/// Create an icon with a colored background and text
pub fn create_icon(text: &str, bg_color: (u8, u8, u8)) -> Icon {
    let mut img = RgbaImage::new(ICON_SIZE, ICON_SIZE);
    
    // Fill background
    let bg = Rgba([bg_color.0, bg_color.1, bg_color.2, 255]);
    for pixel in img.pixels_mut() {
        *pixel = bg;
    }
    
    // Draw white text
    let text_color = Rgba([255, 255, 255, 255]);
    draw_text_centered(&mut img, text, text_color);
    
    // Add a subtle border (darker version of bg color)
    let border = Rgba([
        (bg_color.0 as f32 * 0.7) as u8,
        (bg_color.1 as f32 * 0.7) as u8,
        (bg_color.2 as f32 * 0.7) as u8,
        255,
    ]);
    
    // Top and bottom borders
    for x in 0..ICON_SIZE {
        img.put_pixel(x, 0, border);
        img.put_pixel(x, ICON_SIZE - 1, border);
    }
    // Left and right borders
    for y in 0..ICON_SIZE {
        img.put_pixel(0, y, border);
        img.put_pixel(ICON_SIZE - 1, y, border);
    }
    
    // Convert to tray icon
    let rgba_data = img.into_raw();
    Icon::from_rgba(rgba_data, ICON_SIZE, ICON_SIZE).expect("Failed to create icon")
}

/// Create an offline/error icon (gray with "?")
pub fn create_offline_icon() -> Icon {
    create_icon("?", (128, 128, 128))
}

/// Create a temperature icon
pub fn create_temp_icon(temp: i32, color: (u8, u8, u8)) -> Icon {
    let text = format!("{}", temp);
    create_icon(&text, color)
}

/// Fan max RPM values for each fan
pub const FAN1_MAX_RPM: i32 = 4100;
pub const FAN2_MAX_RPM: i32 = 4300;
pub const FAN3_MAX_RPM: i32 = 1800;

/// Create a fan RPM icon with signal-style bars (like cell phone signal)
/// fan_num: 1, 2, or 3 to determine max RPM
pub fn create_rpm_bar_icon(rpm: i32, fan_num: u8, color: (u8, u8, u8)) -> Icon {
    let max_rpm = match fan_num {
        1 => FAN1_MAX_RPM,
        2 => FAN2_MAX_RPM,
        3 => FAN3_MAX_RPM,
        _ => 4000,
    };
    
    // Calculate how many bars to fill (0-5 bars)
    let percentage = (rpm as f32 / max_rpm as f32).clamp(0.0, 1.0);
    let filled_bars = (percentage * 5.0).ceil() as u32;
    
    let mut img = RgbaImage::new(ICON_SIZE, ICON_SIZE);
    
    // Fill background with dark color
    let bg = Rgba([40, 40, 40, 255]);
    for pixel in img.pixels_mut() {
        *pixel = bg;
    }
    
    // Bar dimensions
    let bar_width: u32 = 2;
    let bar_spacing: u32 = 1;
    let total_width = 5 * bar_width + 4 * bar_spacing; // 5 bars with 4 gaps
    let start_x = (ICON_SIZE - total_width) / 2;
    let bottom_y = ICON_SIZE - 2; // Leave 2px margin at bottom
    
    // Bar heights (increasing like signal bars)
    let bar_heights: [u32; 5] = [4, 6, 8, 10, 12];
    
    let filled_color = Rgba([color.0, color.1, color.2, 255]);
    let empty_color = Rgba([80, 80, 80, 255]); // Dark gray for empty bars
    
    for i in 0..5u32 {
        let bar_x = start_x + i * (bar_width + bar_spacing);
        let bar_height = bar_heights[i as usize];
        let bar_top = bottom_y - bar_height;
        
        let bar_color = if i < filled_bars {
            filled_color
        } else {
            empty_color
        };
        
        // Draw the bar
        for x in bar_x..(bar_x + bar_width) {
            for y in bar_top..bottom_y {
                if x < ICON_SIZE && y < ICON_SIZE {
                    img.put_pixel(x, y, bar_color);
                }
            }
        }
    }
    
    // Add subtle border
    let border = Rgba([60, 60, 60, 255]);
    for x in 0..ICON_SIZE {
        img.put_pixel(x, 0, border);
        img.put_pixel(x, ICON_SIZE - 1, border);
    }
    for y in 0..ICON_SIZE {
        img.put_pixel(0, y, border);
        img.put_pixel(ICON_SIZE - 1, y, border);
    }
    
    let rgba_data = img.into_raw();
    Icon::from_rgba(rgba_data, ICON_SIZE, ICON_SIZE).expect("Failed to create icon")
}

/// Create an offline fan icon (gray bars, all empty)
pub fn create_offline_fan_icon() -> Icon {
    let mut img = RgbaImage::new(ICON_SIZE, ICON_SIZE);
    
    // Fill background
    let bg = Rgba([40, 40, 40, 255]);
    for pixel in img.pixels_mut() {
        *pixel = bg;
    }
    
    // Draw empty gray bars with "?" overlay
    let bar_width: u32 = 2;
    let bar_spacing: u32 = 1;
    let total_width = 5 * bar_width + 4 * bar_spacing;
    let start_x = (ICON_SIZE - total_width) / 2;
    let bottom_y = ICON_SIZE - 2;
    let bar_heights: [u32; 5] = [4, 6, 8, 10, 12];
    let empty_color = Rgba([80, 80, 80, 255]);
    
    for i in 0..5u32 {
        let bar_x = start_x + i * (bar_width + bar_spacing);
        let bar_height = bar_heights[i as usize];
        let bar_top = bottom_y - bar_height;
        
        for x in bar_x..(bar_x + bar_width) {
            for y in bar_top..bottom_y {
                if x < ICON_SIZE && y < ICON_SIZE {
                    img.put_pixel(x, y, empty_color);
                }
            }
        }
    }
    
    // Border
    let border = Rgba([60, 60, 60, 255]);
    for x in 0..ICON_SIZE {
        img.put_pixel(x, 0, border);
        img.put_pixel(x, ICON_SIZE - 1, border);
    }
    for y in 0..ICON_SIZE {
        img.put_pixel(0, y, border);
        img.put_pixel(ICON_SIZE - 1, y, border);
    }
    
    let rgba_data = img.into_raw();
    Icon::from_rgba(rgba_data, ICON_SIZE, ICON_SIZE).expect("Failed to create icon")
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_create_icon() {
        let icon = create_icon("65", (76, 175, 80));
        // Icon creation should not panic
        drop(icon);
    }
    
    #[test]
    fn test_create_rpm_icon() {
        let icon = create_rpm_icon(2400, (255, 193, 7));
        drop(icon);
    }
}
