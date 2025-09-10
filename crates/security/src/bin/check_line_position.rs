#!/usr/bin/env python3

# Check the exact line that contains position 1175
with open('../security/schema/policy_schema.cedarschema', 'rb') as f:
    content = f.read()

print(f"File length: {len(content)}")

# Find which line contains position 1175
newline_positions = []
for i, byte in enumerate(content):
    if byte == ord('\n'):
        newline_positions.append(i)

print(f"Number of newlines: {len(newline_positions)}")

# Find which line contains position 1175
line_number = 0
for i, newline_pos in enumerate(newline_positions):
    if newline_pos > 1175:
        line_number = i
        break
else:
    line_number = len(newline_positions)

print(f"Position 1175 is on line {line_number + 1}")

# Show the actual line content
if line_number < len(newline_positions):
    line_start = newline_positions[line_number - 1] + 1 if line_number > 0 else 0
    line_end = newline_positions[line_number]
    line_content = content[line_start:line_end]
    print(f"Line {line_number + 1} content:")
    print(f"'{line_content.decode('utf-8', errors='replace')}'")
    print(f"Line length: {len(line_content)}")
    print(f"Position 1175 offset in line: {1175 - line_start}")
else:
    print("Position 1175 is on the last line")

# Let's also check if there are any non-printable characters in the file
non_printable_chars = []
for i, byte in enumerate(content[:2000]):  # Check first 2000 bytes
    if byte < 32 and byte not in [9, 10, 13]:  # Exclude tab, newline, carriage return
        non_printable_chars.append((i, byte))

if non_printable_chars:
    print(f"\nFound {len(non_printable_chars)} non-printable characters:")
    for pos, byte in non_printable_chars[:10]:  # Show first 10
        print(f"  Position {pos}: 0x{byte:02x}")
else:
    print("\nNo non-printable characters found in first 2000 bytes")