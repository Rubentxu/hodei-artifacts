import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@shared/test/test-utils';
import Input from '../Input';

describe('Input', () => {
  it('renders basic input with placeholder', () => {
    render(<Input placeholder="Enter text" />);

    const input = screen.getByPlaceholderText('Enter text');
    expect(input).toBeInTheDocument();
  });

  it('renders with different types', () => {
    const { rerender } = render(<Input type="text" placeholder="Text" />);
    expect(screen.getByPlaceholderText('Text')).toHaveAttribute('type', 'text');

    rerender(<Input type="email" placeholder="Email" />);
    expect(screen.getByPlaceholderText('Email')).toHaveAttribute(
      'type',
      'email'
    );

    rerender(<Input type="password" placeholder="Password" />);
    expect(screen.getByPlaceholderText('Password')).toHaveAttribute(
      'type',
      'password'
    );

    rerender(<Input type="number" placeholder="Number" />);
    expect(screen.getByPlaceholderText('Number')).toHaveAttribute(
      'type',
      'number'
    );
  });

  it('handles value changes', () => {
    const handleChange = vi.fn();
    render(<Input placeholder="Test input" onChange={handleChange} />);

    const input = screen.getByPlaceholderText('Test input');
    fireEvent.change(input, { target: { value: 'Hello World' } });

    expect(handleChange).toHaveBeenCalledTimes(1);
    expect(handleChange).toHaveBeenCalledWith(expect.any(Object));
  });

  it('is disabled when disabled prop is true', () => {
    render(<Input placeholder="Disabled" disabled />);

    const input = screen.getByPlaceholderText('Disabled');
    expect(input).toBeDisabled();
  });

  it('renders with error state', () => {
    render(<Input placeholder="Error" error />);

    const input = screen.getByPlaceholderText('Error');
    expect(input).toBeInTheDocument();
  });

  it('applies custom className', () => {
    render(<Input placeholder="Custom" className="custom-input" />);

    const input = screen.getByPlaceholderText('Custom');
    expect(input).toHaveClass('custom-input');
  });

  it('forwards ref correctly', () => {
    const ref = vi.fn();
    render(<Input placeholder="Ref test" ref={ref} />);

    expect(ref).toHaveBeenCalledWith(expect.any(HTMLInputElement));
  });

  it('handles focus and blur events', () => {
    const handleFocus = vi.fn();
    const handleBlur = vi.fn();

    render(
      <Input
        placeholder="Focus test"
        onFocus={handleFocus}
        onBlur={handleBlur}
      />
    );

    const input = screen.getByPlaceholderText('Focus test');

    fireEvent.focus(input);
    expect(handleFocus).toHaveBeenCalledTimes(1);

    fireEvent.blur(input);
    expect(handleBlur).toHaveBeenCalledTimes(1);
  });

  it('renders with different sizes', () => {
    const { rerender } = render(<Input placeholder="Default size" />);
    expect(screen.getByPlaceholderText('Default size')).toBeInTheDocument();

    rerender(<Input placeholder="Small size" size="sm" />);
    expect(screen.getByPlaceholderText('Small size')).toBeInTheDocument();

    rerender(<Input placeholder="Large size" size="lg" />);
    expect(screen.getByPlaceholderText('Large size')).toBeInTheDocument();
  });

  it('handles keyboard events', () => {
    const handleKeyDown = vi.fn();
    const handleKeyUp = vi.fn();

    render(
      <Input
        placeholder="Keyboard test"
        onKeyDown={handleKeyDown}
        onKeyUp={handleKeyUp}
      />
    );

    const input = screen.getByPlaceholderText('Keyboard test');

    fireEvent.keyDown(input, { key: 'Enter', code: 'Enter' });
    expect(handleKeyDown).toHaveBeenCalledTimes(1);

    fireEvent.keyUp(input, { key: 'Enter', code: 'Enter' });
    expect(handleKeyUp).toHaveBeenCalledTimes(1);
  });

  it('renders with prefix and suffix slots when provided', () => {
    render(
      <Input
        placeholder="With slots"
        prefix={<span data-testid="prefix">$</span>}
        suffix={<span data-testid="suffix">USD</span>}
      />
    );

    expect(screen.getByPlaceholderText('With slots')).toBeInTheDocument();
  });

  it('maintains proper accessibility attributes', () => {
    render(
      <Input
        placeholder="Accessible"
        aria-label="Test input field"
        id="test-input"
      />
    );

    const input = screen.getByPlaceholderText('Accessible');
    expect(input).toHaveAttribute('aria-label', 'Test input field');
    expect(input).toHaveAttribute('id', 'test-input');
  });
});
