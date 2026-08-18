import { useState } from "react";

type CustomSelectProps = {
  label: string;
  value: string;
  onChange: (value: string) => void;
  options: { value: string; label: string }[];
  disabled?: boolean;
};

export function CustomSelect({ label, value, onChange, options, disabled }: CustomSelectProps) {
  const [open, setOpen] = useState(false);

  return (
    <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
      <span style={{ fontSize: "10px", opacity: 0.8, textTransform: "uppercase", letterSpacing: "0.5px" }}>{label}:&nbsp;</span>

      <button
        onClick={() => setOpen(!open)}
        style={{
          minHeight: "35px",
          display: "flex",
          alignItems: "center",
          gap: "4px",
          padding: "0 8px",
          borderRadius: "9px",
          background: "var(--surface-input)",
          color: "var(--text)",
          border: "1px solid var(--line)",
          fontSize: "12px",
          cursor: disabled ? "not-allowed" : "pointer",
          ...(disabled && { opacity: 0.45 }),
        }}
      >
        {value}
        <svg
          width="8"
          height="8"
          viewBox="0 0 4 5"
          style={{ transition: "transform 0.15s ease" }}
        >
          <path d="M2 5L0 0L4 0Z" fill="currentColor" />
        </svg>
      </button>

      {open && !disabled && (
        <div
          style={{
            position: "absolute",
            top: 44,
            left: 0,
            right: 0,
            background: "var(--surface-input)",
            border: "1px solid var(--line)",
            borderRadius: "9px",
            padding: "6px 0",
            marginLeft: "-1px",
            marginRight: "-1px",
            zIndex: 100,
            boxShadow: "0 4px 12px rgba(0,0,0,0.15)",
            color: "var(--text)",
            fontSize: "12px",
          }}
        >
          {options.map((opt) => (
            <button
              key={opt.value}
              onClick={() => {
                onChange(opt.value);
                setOpen(false);
              }}
              style={{
                width: "100%",
                padding: "6px 12px",
                background: "transparent",
                color: "var(--text)",
                border: "none",
                borderRadius: "6px",
                fontSize: "12px",
                cursor: "pointer",
              }}
              className="CustomSelect-option"
            >
              {opt.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}