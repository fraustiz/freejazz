//! Embedded PNG icons for the 19 built-in RGB lighting effects

use windows_reactor::EncodedImage;

pub fn effect_icon_bytes(mode: u8) -> Option<&'static [u8]> {
    Some(match mode {
        1 => include_bytes!("../assets/icons/mode_1.png"),
        2 => include_bytes!("../assets/icons/mode_2.png"),
        3 => include_bytes!("../assets/icons/mode_3.png"),
        4 => include_bytes!("../assets/icons/mode_4.png"),
        5 => include_bytes!("../assets/icons/mode_5.png"),
        6 => include_bytes!("../assets/icons/mode_6.png"),
        7 => include_bytes!("../assets/icons/mode_7.png"),
        8 => include_bytes!("../assets/icons/mode_8.png"),
        9 => include_bytes!("../assets/icons/mode_9.png"),
        10 => include_bytes!("../assets/icons/mode_10.png"),
        11 => include_bytes!("../assets/icons/mode_11.png"),
        12 => include_bytes!("../assets/icons/mode_12.png"),
        13 => include_bytes!("../assets/icons/mode_13.png"),
        14 => include_bytes!("../assets/icons/mode_14.png"),
        15 => include_bytes!("../assets/icons/mode_15.png"),
        16 => include_bytes!("../assets/icons/mode_16.png"),
        17 => include_bytes!("../assets/icons/mode_17.png"),
        18 => include_bytes!("../assets/icons/mode_18.png"),
        19 => include_bytes!("../assets/icons/mode_19.png"),
        _ => return None,
    })
}

pub fn effect_icon(mode: u8) -> Option<EncodedImage> {
    effect_icon_bytes(mode).map(EncodedImage::from_static)
}
