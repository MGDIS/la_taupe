use std::io::Cursor;

use image::{DynamicImage, ImageFormat};
use itertools::Itertools;
use leptess::{LepTess, Variable};
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::shapes::{Anchor, Point};

pub fn img_to_string_using_tesseract(img: DynamicImage) -> String {
    let img = increase_image_size_if_needed(img);

    let mut buffer = Cursor::new(Vec::new());
    img.write_to(&mut buffer, ImageFormat::Png).unwrap();
    let bytes = buffer.into_inner();

    let mut lt = LepTess::new(None, "fra").expect("Failed to initialize LepTess");
    lt.set_variable(Variable::TesseditPagesegMode, "12")
        .expect("Failed to set PSM");
    lt.set_variable(Variable::PreserveInterwordSpaces, "1")
        .expect("Failed to set preserve_interword_spaces");
    lt.set_image_from_mem(&bytes)
        .expect("Failed to load image");

    lt.get_utf8_text().expect("Failed to get text from LepTess")
}

pub fn tess_analyze(img: &DynamicImage) -> (String, Option<f32>, Option<Anchor>) {
    let (hocr, doc) = image_to_hocr(img);
    let (mut angle, mut anchor) = (None, None);

    if let Some(el) = iban_el(&doc) {
        angle = find_angle(&hocr, el);
        anchor = to_anchor(&el);
    };

    (hocr, angle, anchor)
}

fn image_to_hocr(img: &DynamicImage) -> (String, Html) {
    let mut buffer = Cursor::new(Vec::new());
    img.write_to(&mut buffer, ImageFormat::Png).unwrap();
    let bytes = buffer.into_inner();

    let mut lt = LepTess::new(None, "fra").expect("Failed to initialize LepTess");
    lt.set_variable(Variable::TesseditPagesegMode, "12")
        .expect("Failed to set PSM");
    lt.set_variable(Variable::PreserveInterwordSpaces, "1")
        .expect("Failed to set preserve_interword_spaces");
    lt.set_image_from_mem(&bytes)
        .expect("Failed to load image");

    let hocr = lt
        .get_hocr_text(0)
        .expect("Failed to get hOCR from LepTess");

    let doc = Html::parse_document(&hocr);

    (hocr, doc)
}

fn find_angle(hocr_string: &str, iban_anchor: ElementRef) -> Option<f32> {
    let angle_regexp = Regex::new(r"textangle (\d+)").ok()?;

    let angle = ElementRef::wrap(*iban_anchor)
        .and_then(|el| el.value().attr("title"))
        .and_then(|title| angle_regexp.captures(title))
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<f32>().ok());

    if angle.is_some() {
        return angle;
    }

    let ocr_lines_count = Regex::new(r#"span class='ocr_line'"#)
        .unwrap()
        .captures_iter(hocr_string)
        .count();

    let angles = angle_regexp
        .captures_iter(hocr_string)
        .filter_map(|caps| caps.get(1))
        .filter_map(|m| m.as_str().parse::<u32>().ok())
        .collect::<Vec<_>>();

    // if the number of lines is less than 4 or the number of angles is less than half the number
    // of lines, we can't determine the angle
    if ocr_lines_count < 4 || angles.len() < ocr_lines_count / 2 {
        return None;
    }

    // most common angle
    let angle = angles
        .iter()
        .copied()
        .counts()
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .map(|(angle, _)| (angle as f32).to_radians());

    angle
}

fn iban_el(doc: &Html) -> Option<ElementRef<'_>> {
    let selector = Selector::parse("span.ocrx_word").unwrap();
    let re_iban = Regex::new(r"(?:^|\s)FR[\dO]").unwrap();

    doc.select(&selector).find(|el| {
        let text = el.text().collect::<Vec<_>>().join("");
        re_iban.is_match(&text)
    })
}

fn to_anchor(iban_anchor: &ElementRef) -> Option<Anchor> {
    let title = iban_anchor.value().attr("title")?;

    let re =
        Regex::new(r"bbox (?P<coordinates>\d+ \d+ \d+ \d+).*?x_wconf (?P<confiance>\d+)").ok()?;

    let caps = re.captures(title)?;

    let coord_str = caps.name("coordinates")?.as_str();

    let coords: Vec<u32> = coord_str
        .split_whitespace()
        .map(|s| s.parse::<u32>())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;

    if coords.len() == 4 {
        Some(Anchor::new(
            Point::new(coords[0], coords[1]),
            Point::new(coords[2], coords[3]),
        ))
    } else {
        None
    }
}

fn increase_image_size_if_needed(img: DynamicImage) -> DynamicImage {
    // si la largeur ou la hauteur est inférieur a 500 on multiply par 2
    if img.width() >= 500 && img.height() >= 500 {
        return img;
    }

    // increase * 2 if the image is too small
    img.resize(
        img.width() * 2,
        img.height() * 2,
        image::imageops::FilterType::Lanczos3,
    )
}
