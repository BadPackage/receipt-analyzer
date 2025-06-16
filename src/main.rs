use anyhow::{Context, Result};
use clap::Parser;
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
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
    let mut tesseract = Tesseract::new(None, Some("eng"))?
        .set_image(image_path.to_str().unwrap())?;
    let text = tesseract.get_text()?;

    parse_receipt_text(&text)
}

fn parse_receipt_text(text: &str) -> Result<Vec<Product>> {
    let mut products = Vec::new();

    // Regex patterns for different receipt formats
    // Pattern 1: Product name followed by price (with $ or without)
    let pattern1 = Regex::new(r"([A-Za-z][A-Za-z0-9\s\-.]{2,30})\s+\$?(\d+\.\d{2})")?;

    // Pattern 2: Price at end of line with possible product name before
    let pattern2 = Regex::new(r"^(.+?)\s+(\d{1,4}\.\d{2})$")?;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.len() < 4 {
            continue;
        }

        // Skip lines that look like headers, totals, taxes, etc.
        if line.to_lowercase().contains("total")
            || line.to_lowercase().contains("tax")
            || line.to_lowercase().contains("subtotal")
            || line.to_lowercase().contains("change")
            || line.to_lowercase().contains("receipt")
            || line.to_lowercase().contains("date")
            || line.to_lowercase().contains("time")
        {
            continue;
        }

        // Try pattern 1 first
        if let Some(captures) = pattern1.captures(line) {
            if let (Some(name), Some(price_str)) = (captures.get(1), captures.get(2)) {
                if let Ok(price) = price_str.as_str().parse::<f64>() {
                    if price > 0.0 && price < 1000.0 { // Reasonable price range
                        products.push(Product {
                            name: clean_product_name(name.as_str()),
                            price,
                        });
                    }
                }
            }
        }
        // Try pattern 2 if pattern 1 didn't match
        else if let Some(captures) = pattern2.captures(line) {
            if let (Some(name), Some(price_str)) = (captures.get(1), captures.get(2)) {
                if let Ok(price) = price_str.as_str().parse::<f64>() {
                    if price > 0.0 && price < 1000.0 {
                        let name_str = name.as_str().trim();
                        if name_str.len() > 2 && !name_str.chars().all(|c| c.is_numeric() || c == '.' || c == '-') {
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

fn clean_product_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
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
            Cell::new(&format!("${:.2}", price)),
        ]));
        grand_total += price;
    }

    table.add_row(Row::new(vec![
        Cell::new("TOTAL"),
        Cell::new(&format!("${:.2}", grand_total)).style_spec("b"),
    ]));

    table.printstd();
    println!("\nFound {} unique products", products.len());
}
