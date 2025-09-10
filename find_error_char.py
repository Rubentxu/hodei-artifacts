#!/usr/bin/env python3

# Find the exact character at position 1175 in the schema file
with open('crates/security/schema/policy_schema.cedarschema', 'rb') as f:
    content = f.read()

print(f"File length: {len(content)}")
print(f"Character at position 1175: {chr(content[1175])}")
print(f"Byte value: {content[1175]}")

# Show context around position 1175
start = max(0, 1175 - 50)
end = min(len(content), 1175 + 50)
context = content[start:end]

print(f"\nContext around position 1175:")
print(context.decode('utf-8', errors='replace'))

# Show each character with position
print(f"\nCharacters around position 1175:")
for i in range(max(0, 1175 - 10), min(len(content), 1175 + 11)):
    char = chr(content[i])
    if i == 1175:
        print(f"Position {i}: '{char}' <<<< ERROR HERE")
    else:
        print(f"Position {i}: '{char}'")