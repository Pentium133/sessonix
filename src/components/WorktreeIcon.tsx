interface WorktreeIconProps {
  className?: string;
  size?: number;
  title?: string;
}

export default function WorktreeIcon({
  className,
  size = 12,
  title,
}: WorktreeIconProps) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="currentColor"
      aria-hidden={title ? undefined : true}
      role={title ? "img" : undefined}
    >
      {title ? <title>{title}</title> : null}
      <path d="M8 1.5 10.7 5H9.4l3 3.8H9.5v2.7H11V13H5v-1.5h1.5V8.8H3.6l3-3.8H5.3L8 1.5Z" />
    </svg>
  );
}
