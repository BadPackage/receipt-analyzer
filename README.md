# Receipt Analyzer CLI

A minimal Rust CLI tool that analyzes receipt images using OCR and extracts product prices with fuzzy matching for error correction.

## Prerequisites

1. **Install Tesseract OCR**:
   ```bash
   # Ubuntu/Debian
   sudo apt install tesseract-ocr tesseract-ocr-eng libtesseract-dev

   # macOS
   brew install tesseract
  
   # Fedora
   sudo dnf install tesseract tesseract-langpack-eng tesseract-devel
   
   # Windows
   # Download and install from: https://github.com/UB-Mannheim/tesseract/wiki
   ```

2. **Install Rust** (if not already installed):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

## Build

```bash
cargo build --release
```

## Usage

```bash
# Run on a directory containing receipt images
cargo run -- --dir /path/to/receipt/images

# Or after building
./target/release/receipt-analyzer --dir /path/to/receipt/images
```

## Features

- **OCR Processing**: Uses Tesseract for precise text recognition
- **Fuzzy Matching**: Corrects OCR errors by matching similar product names
- **Multi-Receipt Support**: Processes all images in a directory and sums up identical products
- **Smart Parsing**: Filters out totals, taxes, and other non-product lines
- **Sorted Output**: Results sorted by total price (descending)
- **DE Decimal Format**: Uses standard US pricing format (€XX,XX)

## Supported Image Formats

- JPG/JPEG
- PNG
- TIFF
- BMP

## Example Output

```
Processing: receipts/receipt1.jpg
Processing: receipts/receipt2.png

+------------------+-------------+
| Product Name     | Total Price |
+------------------+-------------+
| milk whole       | $8.97       |
| bread wheat      | $5.48       |
| eggs large       | $4.99       |
| apples red       | $3.50       |
+------------------+-------------+
| TOTAL            | $22.94      |
+------------------+-------------+

Found 4 unique products
```

## Notes

- The fuzzy matching threshold is set to 80% similarity
- Products with prices over $1000 are filtered out as likely OCR errors
- Product names are normalized (lowercase, alphanumeric only) for better matching