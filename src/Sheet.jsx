import { useState } from 'react';
import { CloseIcon } from './Icons.jsx';

// Shared building blocks for the drawers (Map Options, History, Receiver,
// Settings), so they all look the same.

// A drawer that slides in from the right edge of the window.
export function Sheet({ title, icon, onClose, children }) {
  return (
    <aside className="sheet">
      <div className="sheet-title">
        {icon && <span className="sheet-icon">{icon}</span>}
        <span>{title}</span>
        <button type="button" className="sheet-close" onClick={onClose} aria-label="Close">
          <CloseIcon />
        </button>
      </div>
      <div className="sheet-body">{children}</div>
    </aside>
  );
}

export function SheetSection({ title, children }) {
  return (
    <section className="sheet-section">
      {title && <h4>{title}</h4>}
      {children}
    </section>
  );
}

// Label (and optional hint) on the left, control on the right.
export function SheetRow({ label, hint, children, htmlFor }) {
  return (
    <div className="sheet-row">
      <label htmlFor={htmlFor}>
        <span>{label}</span>
        {hint && <small>{hint}</small>}
      </label>
      <div className="sheet-control">{children}</div>
    </div>
  );
}

export function Switch({ id, checked, onChange, disabled }) {
  return (
    <input
      id={id}
      type="checkbox"
      role="switch"
      className="switch"
      disabled={disabled}
      checked={checked}
      onChange={(e) => onChange(e.target.checked)}
    />
  );
}

export function LinkButton({ children, onClick, danger, disabled }) {
  return (
    <button
      type="button"
      className={`link-btn${danger ? ' danger' : ''}`}
      onClick={onClick}
      disabled={disabled}
    >
      {children}
    </button>
  );
}

// A row of mutually exclusive options (like iOS segmented controls).
export function Segmented({ label, options, value, onChange }) {
  return (
    <div className="seg-row">
      <span className="seg-label">{label}</span>
      <div className="segmented" role="radiogroup" aria-label={label}>
        {options.map(([optValue, optLabel]) => (
          <button
            key={optValue}
            type="button"
            role="radio"
            aria-checked={value === optValue}
            className={value === optValue ? 'on' : undefined}
            onClick={() => onChange(optValue)}
          >
            {optLabel}
          </button>
        ))}
      </div>
    </div>
  );
}

// A card that expands to show its options (Map Options sections).
export function Collapsible({ title, children, defaultOpen = false }) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <section className={`collapsible${open ? ' open' : ''}`}>
      <button type="button" className="collapsible-head" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
        <span>{title}</span>
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
          <path d="m6 9 6 6 6-6" />
        </svg>
      </button>
      {open && <div className="collapsible-body">{children}</div>}
    </section>
  );
}
