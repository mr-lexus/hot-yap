import type { ReactNode, SVGProps } from "react";

export type IconName =
  | "check"
  | "chevron"
  | "clipboard"
  | "close"
  | "cpu"
  | "download"
  | "help"
  | "keyboard"
  | "layers"
  | "lock"
  | "maximize"
  | "mic"
  | "minimize"
  | "moon"
  | "play"
  | "refresh"
  | "restore"
  | "search"
  | "shield"
  | "sliders"
  | "stop"
  | "sun"
  | "trash"
  | "waveform";

interface IconProps extends SVGProps<SVGSVGElement> {
  name: IconName;
  size?: number;
}

export default function Icon({ name, size = 16, ...props }: IconProps) {
  const paths: Record<IconName, ReactNode> = {
    check: <path d="m5 12 4 4L19 6" />,
    chevron: <path d="m9 18 6-6-6-6" />,
    clipboard: <><rect x="6" y="4" width="12" height="16" rx="2" /><path d="M9 4.5h6V7H9z" /></>,
    close: <><path d="m7 7 10 10" /><path d="M17 7 7 17" /></>,
    cpu: <><rect x="7" y="7" width="10" height="10" rx="2" /><path d="M9 1v3M15 1v3M9 20v3M15 20v3M20 9h3M20 14h3M1 9h3M1 14h3" /></>,
    download: <><path d="M12 3v12" /><path d="m7 10 5 5 5-5" /><path d="M5 20h14" /></>,
    help: <><path d="M9.5 9a2.75 2.75 0 1 1 4.1 2.4c-1.1.6-1.6 1.1-1.6 2.1" /><path d="M12 18h.01" /><circle cx="12" cy="12" r="9" /></>,
    keyboard: <><rect x="3" y="6" width="18" height="12" rx="2" /><path d="M7 10h.01M11 10h.01M15 10h.01M18 10h.01M7 14h10" /></>,
    layers: <><path d="m12 3 9 5-9 5-9-5 9-5Z" /><path d="m3 12 9 5 9-5M3 16l9 5 9-5" /></>,
    lock: <><rect x="5" y="10" width="14" height="11" rx="2" /><path d="M8 10V7a4 4 0 0 1 8 0v3" /></>,
    maximize: <rect x="4.5" y="4.5" width="15" height="15" rx="1.5" />,
    mic: <><rect x="9" y="3" width="6" height="12" rx="3" /><path d="M5 11a7 7 0 0 0 14 0M12 18v3M9 21h6" /></>,
    minimize: <path d="M4.5 12h15" />,
    moon: <path d="M20 15.5A8.5 8.5 0 0 1 8.5 4 8.5 8.5 0 1 0 20 15.5Z" />,
    play: <path d="m9 6 9 6-9 6V6Z" />,
    refresh: <><path d="M20 7v5h-5" /><path d="M4 17v-5h5" /><path d="M6.1 8a7 7 0 0 1 11.4-2.1L20 8M4 16l2.5 2.1A7 7 0 0 0 17.9 16" /></>,
    restore: <><rect x="4.5" y="9.5" width="10" height="10" rx="1.5" /><path d="M9.5 4.5h10v10" /></>,
    search: <><circle cx="10.5" cy="10.5" r="6.5" /><path d="m16 16 5 5" /></>,
    shield: <><path d="M12 3 20 6v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6l8-3Z" /><path d="m9 12 2 2 4-4" /></>,
    sliders: <><path d="M4 6h10M18 6h2M4 12h2M10 12h10M4 18h7M15 18h5" /><circle cx="16" cy="6" r="2" /><circle cx="8" cy="12" r="2" /><circle cx="13" cy="18" r="2" /></>,
    stop: <rect x="7" y="7" width="10" height="10" rx="2" />,
    sun: <><circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" /></>,
    trash: <><path d="M4 7h16M9 7V4h6v3M7 7l1 14h8l1-14M10 11v6M14 11v6" /></>,
    waveform: <path d="M3 12h2l2-5 3 10 3-13 3 16 2-8h3" />,
  };

  return (
    <svg
      aria-hidden="true"
      fill="none"
      height={size}
      viewBox="0 0 24 24"
      width={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.8"
      {...props}
    >
      {paths[name]}
    </svg>
  );
}
