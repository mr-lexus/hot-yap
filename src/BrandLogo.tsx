interface BrandLogoProps {
  size?: number;
  variant: "dark" | "light";
}

export default function BrandLogo({ size = 28, variant }: BrandLogoProps) {
  return (
    <img
      className="brand-logo"
      alt=""
      aria-hidden="true"
      draggable={false}
      height={size}
      src={`/${variant}.png`}
      width={size}
    />
  );
}
