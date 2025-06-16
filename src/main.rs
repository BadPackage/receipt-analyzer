use anyhow::{Context, Result};
use clap::Parser;
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use image::{ImageBuffer, Luma, DynamicImage};
use prettytable::{format, Cell, Row, Table};
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use tesseract::Tesseract;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "receipt-analyzer")]
#[command(about = "Analyze receipt images and extract product prices")]
struct Args {
    /// Directory containing receipt images
    #[arg(short, long)]
    dir: String,
}

#[derive(Debug)]
struct Product {
    name: String,
    price: f64,
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("Analyzing receipts in: {}", args.dir);

    let products = process_receipt_directory(&args.dir)?;
    let aggregated = aggregate_products(products);
    display_results(aggregated);

    Ok(())
}

fn process_receipt_directory(dir_path: &str) -> Result<Vec<Product>> {
    let mut all_products = Vec::new();
    let image_extensions = ["jpg", "jpeg", "png", "tiff", "bmp"];

    for entry in WalkDir::new(dir_path) {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        if let Some(ext) = path.extension() {
            if image_extensions.contains(&ext.to_str().unwrap_or("").to_lowercase().as_str()) {
                println!("Processing: {}", path.display());

                match extract_products_from_image(path) {
                    Ok(mut products) => {
                        all_products.append(&mut products);
                    }
                    Err(e) => {
                        eprintln!("Error processing {}: {}", path.display(), e);
                    }
                }
            }
        }
    }

    Ok(all_products)
}

fn extract_products_from_image(image_path: &Path) -> Result<Vec<Product>> {
    // Load and preprocess image for better OCR
    let img = image::open(image_path)?;
    let processed_img = preprocess_image(img);

    // Save processed image temporarily
    let temp_path = format!("/tmp/processed_{}", image_path.file_name().unwrap().to_str().unwrap());
    processed_img.save(&temp_path)?;

    // Use German language for better OCR on German receipts
    let mut tesseract = Tesseract::new(None, Some("deu+eng"))?
        .set_image(&temp_path)?;

    let text = tesseract.get_text()?;

    // Clean up temp file
    std::fs::remove_file(&temp_path).ok();

    parse_receipt_text(&text)
}

fn preprocess_image(img: DynamicImage) -> DynamicImage {
    // Convert to grayscale
    let gray = img.to_luma8();

    // Increase contrast
    let enhanced = enhance_contrast(gray);

    DynamicImage::ImageLuma8(enhanced)
}

fn enhance_contrast(img: ImageBuffer<Luma<u8>, Vec<u8>>) -> ImageBuffer<Luma<u8>, Vec<u8>> {
    let mut enhanced = img.clone();

    for pixel in enhanced.pixels_mut() {
        let value = pixel[0] as f32;
        // Apply contrast enhancement
        let new_value = ((value - 128.0) * 1.5 + 128.0).clamp(0.0, 255.0) as u8;
        pixel[0] = new_value;
    }

    enhanced
}

fn parse_receipt_text(text: &str) -> Result<Vec<Product>> {
    let mut products = Vec::new();

    #[cfg(debug_assertions)]
    println!("OCR Text:\n{}\n---", text); // Debug output

    // Enhanced patterns for multiple receipt formats
    // Pattern 1: German format with quantity and total - "4x Löwenbräu Original a 3,00 12,00"
    let pattern_qty_total = Regex::new(r"(\d+|[IilL])x?\s+([A-Za-zÄÖÜäöüß][A-Za-zÄÖÜäöüß0-9\s\-.]{2,40})\s+(?:a\s+)?(?:\d+[,.]\d{2}\s+)?(\d+[,.]\d{2})")?;

    // Pattern 2: Euro format - "1 CHICKEN HEALS €9.99" or "2° PIZZA €25.98"
    let pattern_euro = Regex::new(r"(\d+)°?\s+([A-Z][A-Z0-9\s\-.]{2,30})\s+€(\d+[,.]?\d{2})")?;

    // Pattern 3: Simple product line - "EXTRA SPYCIES €0.00"
    let pattern_euro_simple = Regex::new(r"([A-Z][A-Z0-9\s\-.]{2,30})\s+€(\d+[,.]?\d{2})")?;

    // Pattern 4: German simple - "1 Cheeseburger* 1,19"
    let pattern_de_simple = Regex::new(r"(\d+|[IilL])x?\s+([A-Za-zÄÖÜäöüß][A-Za-zÄÖÜäöüß0-9\s\-.*]{2,30})\s+(\d+[,.]\d{2})")?;

    // Pattern 5: Product name followed by price - fallback
    let pattern_fallback = Regex::new(r"([A-Za-zÄÖÜäöüß][A-Za-zÄÖÜäöüß0-9\s\-.]{2,30})\s+(\d+[,.]\d{2})")?;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.len() < 4 {
            continue;
        }

        // Skip headers, totals, taxes, etc.
        if should_skip_line(line) {
            continue;
        }

        // Try patterns in order of specificity
        if let Some(captures) = pattern_qty_total.captures(line) {
            if let (Some(qty_str), Some(name), Some(price_str)) =
                (captures.get(1), captures.get(2), captures.get(3)) {
                // Handle OCR errors: "Ix" -> "1"
                parse_quantity(qty_str.as_str());
                if let Ok(price) = parse_european_price(price_str.as_str()) {
                    if price > 0.0 && price < 1000.0 {
                        products.push(Product {
                            name: clean_product_name(name.as_str()),
                            price,
                        });
                    }
                }
            }
        }
        else if let Some(captures) = pattern_euro.captures(line) {
            if let (Some(_qty_str), Some(name), Some(price_str)) =
                (captures.get(1), captures.get(2), captures.get(3)) {
                if let Ok(price) = parse_european_price(price_str.as_str()) {
                    if price > 0.0 && price < 1000.0 {
                        products.push(Product {
                            name: clean_product_name(name.as_str()),
                            price,
                        });
                    }
                }
            }
        }
        else if let Some(captures) = pattern_euro_simple.captures(line) {
            if let (Some(name), Some(price_str)) = (captures.get(1), captures.get(2)) {
                if let Ok(price) = parse_european_price(price_str.as_str()) {
                    if price > 0.0 && price < 1000.0 {
                        products.push(Product {
                            name: clean_product_name(name.as_str()),
                            price,
                        });
                    }
                }
            }
        }
        else if let Some(captures) = pattern_de_simple.captures(line) {
            if let (Some(_qty_str), Some(name), Some(price_str)) =
                (captures.get(1), captures.get(2), captures.get(3)) {
                if let Ok(price) = parse_european_price(price_str.as_str()) {
                    if price > 0.0 && price < 1000.0 {
                        products.push(Product {
                            name: clean_product_name(name.as_str()),
                            price,
                        });
                    }
                }
            }
        }
        else if let Some(captures) = pattern_fallback.captures(line) {
            if let (Some(name), Some(price_str)) = (captures.get(1), captures.get(2)) {
                if let Ok(price) = parse_european_price(price_str.as_str()) {
                    if price > 0.0 && price < 1000.0 {
                        let name_str = name.as_str().trim();
                        if name_str.len() > 2 && !name_str.chars().all(|c| c.is_numeric() || c == '.' || c == ',' || c == '-') {
                            products.push(Product {
                                name: clean_product_name(name_str),
                                price,
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(products)
}

fn parse_quantity(qty_str: &str) -> u32 {
    // Handle OCR errors where "1x" becomes "Ix", "lx", etc.
    match qty_str.to_lowercase().as_str() {
        "ix" | "lx" | "i" | "l" => 1,
        _ => qty_str.parse().unwrap_or(1)
    }
}

fn should_skip_line(line: &str) -> bool {
    let line_lower = line.to_lowercase();
    line_lower.contains("total") ||
        line_lower.contains("subtotal") ||
        line_lower.contains("summe") ||
        line_lower.contains("netto") ||
        line_lower.contains("brutto") ||
        line_lower.contains("mwst") ||
        line_lower.contains("tax") ||
        line_lower.contains("steuer") ||
        line_lower.contains("change") ||
        line_lower.contains("wechselgeld") ||
        line_lower.contains("receipt") ||
        line_lower.contains("quittung") ||
        line_lower.contains("rechnung") ||
        line_lower.contains("datum") ||
        line_lower.contains("date") ||
        line_lower.contains("time") ||
        line_lower.contains("uhrzeit") ||
        line_lower.contains("tel:") ||
        line_lower.contains("telefon") ||
        line_lower.contains("adresse") ||
        line_lower.contains("address") ||
        line_lower.contains("vielen dank") ||
        line_lower.contains("danke") ||
        line_lower.contains("nr.") ||
        line_lower.contains("nummer") ||
        line_lower.contains("check:") ||
        line_lower.contains("authorization") ||
        line_lower.contains("approval") ||
        line_lower.contains("payment") ||
        line_lower.contains("card") ||
        line_lower.contains("gratuity") ||
        line_lower.contains("signature") ||
        line_lower.contains("customer copy") ||
        line_lower.contains("thanks") ||
        line_lower.starts_with("#") ||
        line_lower.starts_with("<<<") ||
        line_lower.starts_with("888") ||
        line_lower.contains("inkl") ||
        line_lower.contains("gegeben") ||
        line_lower.contains("euro0") ||
        line_lower.contains("eur0") ||
        line_lower.contains("cust:") ||
        line_lower.contains("albany") ||
        line_lower.contains("street") ||
        line_lower.contains("nyc") ||
        line_lower.contains("food club") ||
        // Skip percentage lines
        line_lower.contains("%") ||
        // Skip lines that are just numbers
        line.chars().all(|c| c.is_numeric() || c.is_whitespace())
}

fn parse_european_price(price_str: &str) -> Result<f64, std::num::ParseFloatError> {
    // Handle both European (1,19) and US (1.19) decimal formats
    if price_str.contains(',') {
        // European format: replace comma with dot
        price_str.replace(',', ".").parse::<f64>()
    } else {
        // US format: parse directly
        price_str.parse::<f64>()
    }
}

fn clean_product_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        // Keep German umlauts and special characters
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || "äöüßÄÖÜ".contains(*c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn aggregate_products(products: Vec<Product>) -> Vec<(String, f64)> {
    let mut aggregated: HashMap<String, f64> = HashMap::new();
    let matcher = SkimMatcherV2::default();

    for product in products {
        let mut found_match = false;
        let mut best_match_key = String::new();
        let mut best_score = 0;

        // Try to find existing similar product name
        for existing_key in aggregated.keys() {
            if let Some(score) = matcher.fuzzy_match(existing_key, &product.name) {
                if score > 80 && score > best_score { // Threshold for fuzzy matching
                    best_score = score;
                    best_match_key = existing_key.clone();
                    found_match = true;
                }
            }
        }

        if found_match {
            *aggregated.get_mut(&best_match_key).unwrap() += product.price;
        } else {
            aggregated.insert(product.name, product.price);
        }
    }

    // Sort by price descending
    let mut sorted: Vec<_> = aggregated.into_iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    sorted
}

fn display_results(products: Vec<(String, f64)>) {
    if products.is_empty() {
        println!("No products found in receipt images.");
        return;
    }

    let mut table = Table::new();
    table.set_format(*format::consts::FORMAT_BORDERS_ONLY);
    table.set_titles(Row::new(vec![
        Cell::new("Product Name"),
        Cell::new("Total Price"),
    ]));

    let mut grand_total = 0.0;

    for (name, price) in &products {
        table.add_row(Row::new(vec![
            Cell::new(name),
            Cell::new(&format!("€{:.2}", price)),
        ]));
        grand_total += price;
    }

    table.add_row(Row::new(vec![
        Cell::new("TOTAL"),
        Cell::new(&format!("€{:.2}", grand_total)).style_spec("b"),
    ]));

    table.printstd();
    println!("\nFound {} unique products", products.len());
}
