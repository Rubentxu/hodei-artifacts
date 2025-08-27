import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import Button from '../Button';

describe('Button - Basic Tests', () => {
  it('renders button with text', () => {
    render(<Button>Test Button</Button>);
    expect(screen.getByText('Test Button')).toBeDefined();
  });

  it('button is enabled by default', () => {
    render(<Button>Test Button</Button>);
    expect(screen.getByText('Test Button')).not.toBeDisabled();
  });

  it('button is disabled when disabled prop is true', () => {
    render(<Button disabled>Disabled Button</Button>);
    expect(screen.getByText('Disabled Button')).toBeDisabled();
  });
});
