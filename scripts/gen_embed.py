#!/usr/bin/env python3

import sys

def generate_embed(input_file, output_file, var_name):
    with open(input_file, 'rb') as f:
        data = f.read()
    
    with open(output_file, 'w') as f:
        f.write(f'unsigned char {var_name}[] = {{\n')
        for i in range(0, len(data), 12):
            chunk = data[i:i+12]
            hex_str = ', '.join(f'0x{b:02X}' for b in chunk)
            if i + 12 < len(data):
                f.write(f'  {hex_str},\n')
            else:
                f.write(f'  {hex_str}\n')
        f.write('};\n')
        f.write(f'unsigned int {var_name}_len = {len(data)};\n')

if __name__ == '__main__':
    if len(sys.argv) != 4:
        print(f'Usage: {sys.argv[0]} <input_binary> <output_c> <variable_name>')
        sys.exit(1)
    
    generate_embed(sys.argv[1], sys.argv[2], sys.argv[3])
    print(f'Generated {sys.argv[2]} ({sys.argv[3]}: {sys.path[0]})')
