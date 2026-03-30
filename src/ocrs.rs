use image::DynamicImage;
use ocrs::{ImageSource, OcrEngine, TextItem, TextLine};
use regex::Regex;

use crate::shapes::{Anchor, Point};

pub fn image_to_string_using_ocrs(engine: &OcrEngine, img: DynamicImage) -> String {
    let img = img.into_rgb8();

    let img_source = ImageSource::from_bytes(img.as_raw(), img.dimensions()).unwrap();
    let ocr_input = engine.prepare_input(img_source).unwrap();

    let word_rects = engine.detect_words(&ocr_input).unwrap();
    let line_rects = engine.find_text_lines(&ocr_input, &word_rects);
    let line_texts = engine.recognize_text(&ocr_input, &line_rects).unwrap();

    line_texts
        .iter()
        .flatten()
        .filter(|l| l.to_string().len() > 1)
        .map(|l| l.to_string())
        .collect::<Vec<String>>()
        .join("\n")
}

pub fn ocrs_anchors(
    engine: &OcrEngine,
    img: &DynamicImage,
    word_regex: &Regex,
    line_regex: Option<&Regex>,
) -> (String, Vec<TextLine>, Vec<Anchor>) {
    let img = img.clone().into_rgb8();

    let img_source = ImageSource::from_bytes(img.as_raw(), img.dimensions()).unwrap();
    let ocr_input = engine.prepare_input(img_source).unwrap();

    let word_rects = engine.detect_words(&ocr_input).unwrap();
    let line_rects = engine.find_text_lines(&ocr_input, &word_rects);

    let text_lines = engine
        .recognize_text(&ocr_input, &line_rects)
        .unwrap()
        .iter()
        .flatten()
        .cloned()
        .collect::<Vec<TextLine>>();

    let text = text_lines
        .iter()
        .filter(|l| l.to_string().len() > 1)
        .map(|l| l.to_string())
        .collect::<Vec<String>>()
        .join("\n");

    (
        text,
        text_lines.clone(),
        extract_anchors(text_lines, word_regex, line_regex),
    )
}

pub fn extract_anchors(
    text_lines: Vec<TextLine>,
    word_regex: &Regex,
    line_regex: Option<&Regex>,
) -> Vec<Anchor> {
    text_lines
        .iter()
        .filter(|line| {
            if line_regex.is_none() {
                return true;
            }
            line_regex.unwrap().is_match(&line.to_string())
        })
        .flat_map(|line| line.words())
        .filter(|word| word_regex.is_match(&word.to_string()))
        .map(|word| {
            let [p1, _, p3, _, ..] = word
                .rotated_rect()
                .corners()
                .map(|point| [point.x.round() as u32, point.y.round() as u32]);

            Anchor::new(Point::new(p3[0], p3[1]), Point::new(p1[0], p1[1]))
        })
        .collect::<Vec<Anchor>>()
}
