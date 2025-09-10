#!/usr/bin/env python3

# Check exact bytes at position 1175 using different approach
with open('crates/security/schema/policy_schema.cedarschema', 'rb') as f:
    content = f.read()

print(f"File length: {len(content)}")

# Check bytes around position 1175
start = max(0, 1175 - 5)
end = min(len(content), 1175 + 15)
print(f"Bytes around position 1175:")
for i in range(start, end):
    byte_val = content[i]
    char = chr(byte_val) if 32 <= byte_val <= 126 else f'\\x{byte_val:02x}'
    marker = " <<<<" if i == 1175 else ""
    print(f"Position {i:4d}: 0x{byte_val:02x} ({byte_val:3d}) '{char}'{marker}")

# Show the actual line that contains position 1175
line_start = content.rfind(b'\n', 0, 1175) + 1
line_end = content.find(b'\n', 1175)
if line_end == -1:
    line_end = len(content)

line_content = content[line_start:line_end]
print(f"\nLine containing position 1175:")
print(f"'{line_content.decode('utf-8', errors='replace')}'")

# Show offset within the line
offset_in_line = 1175 - line_start
print(f"Offset within line: {offset_in_line}")