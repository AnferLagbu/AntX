#!/usr/bin/env python3
"""
Binary to C source converter for AntX kernel embedding.

Usage: python3 gen_embed.py <input_binary> <output_c_file> <symbol_name>

This script converts a binary file into a C source file containing
the binary data as a byte array, suitable for embedding in the kernel.
"""

import sys
import os

def generate_c_source(input_file, output_file, symbol_name):
    """Convert binary file to C source with byte array."""
    
    if not os.path.exists(input_file):
        print(f"Error: Input file '{input_file}' not found")
        sys.exit(1)
    
    with open(input_file, 'rb') as f:
        binary_data = f.read()
    
    with open(output_file, 'w') as f:
        f.write(f"/* Auto-generated from {os.path.basename(input_file)} */\n")
        f.write(f"#include <stdint.h>\n\n")
        f.write(f"const uint8_t {symbol_name}[] = {{\n")
        
        for i, byte in enumerate(binary_data):
            if i % 16 == 0:
                f.write("    ")
            f.write(f"0x{byte:02x}")
            if i < len(binary_data) - 1:
                f.write(", ")
            if i % 16 == 15:
                f.write("\n")
        
        if len(binary_data) % 16 != 0:
            f.write("\n")
        
        f.write("};\n\n")
        f.write(f"const unsigned int {symbol_name}_len = {len(binary_data)};\n")
    
    print(f"Generated {output_file}: {len(binary_data)} bytes as {symbol_name}")

if __name__ == "__main__":
    if len(sys.argv) != 4:
        print("Usage: python3 gen_embed.py <input_binary> <output_c_file> <symbol_name>")
        sys.exit(1)
    
    input_file = sys.argv[1]
    output_file = sys.argv[2]
    symbol_name = sys.argv[3]
    
    generate_c_source(input_file, output_file, symbol_name)
