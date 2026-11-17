/**
 * The confirmation of an action that cannot be undone.
 *
 * The dialog asks once more, says what is going to happen, and only then
 * offers the button. The confirmation is not a formality: promoting a
 * content to the trusted channel and revoking a concession change what
 * the service serves to everybody who reads that repository, and neither
 * of them can be undone by the person who did it from a distance.
 *
 * The dialog keeps the keyboard out of the way of the people who use it:
 * the focus goes inside when it opens, it stays inside while it is open,
 * and the escape key closes it without confirming anything.
 */

import { useEffect, useRef } from 'react';

import { Button } from '@/components/ui/Button';
import {
  Modal,
  ModalContent,
  ModalDescription,
  ModalFooter,
  ModalHeader,
  ModalTitle,
} from '@/components/ui/Modal';

export interface ConfirmDialogProps {
  open: boolean;
  title: string;
  description: string;
  /** The label of the button that confirms. */
  confirmLabel: string;
  /** Whether the button that confirms is the dangerous one. */
  destructive?: boolean;
  /** Whether the confirmation is waiting for the service. */
  pending?: boolean;
  /** Whether the action cannot be done. */
  disabled?: boolean;
  /** What is inside the dialog, under the description. */
  children?: React.ReactNode;
  onConfirm: () => void;
  onClose: () => void;
}

export const ConfirmDialog = ({
  open,
  title,
  description,
  confirmLabel,
  destructive = false,
  pending = false,
  disabled = false,
  children,
  onConfirm,
  onClose,
}: ConfirmDialogProps) => {
  const content = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const focusable = content.current?.querySelector<HTMLElement>(
      'button:not([disabled]), input, textarea, select, [href]'
    );
    focusable?.focus();
  }, [open]);

  return (
    <Modal open={open} onOpenChange={(next: boolean) => !next && onClose()}>
      <ModalContent
        ref={content}
        onKeyDown={event => {
          if (event.key === 'Enter' && !disabled && !pending) {
            event.preventDefault();
            onConfirm();
          }
        }}
      >
        <ModalHeader>
          <ModalTitle>{title}</ModalTitle>
          <ModalDescription>{description}</ModalDescription>
        </ModalHeader>

        {children && <div className="text-sm text-gray-700">{children}</div>}

        <ModalFooter>
          <Button variant="outline" onClick={onClose} disabled={pending}>
            Cancel
          </Button>
          <Button
            variant={destructive ? 'destructive' : 'primary'}
            onClick={onConfirm}
            isLoading={pending}
            disabled={disabled || pending}
          >
            {confirmLabel}
          </Button>
        </ModalFooter>
      </ModalContent>
    </Modal>
  );
};
