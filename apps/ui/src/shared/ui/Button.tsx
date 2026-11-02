import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cx } from "../lib/cx";
import { Icon } from "./Icon";

export interface ButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "className"> {
  variant?: "default" | "primary" | "ghost" | "danger";
  size?: "sm" | "md";
  /** Leading icon name. */
  icon?: string;
  className?: string;
  children?: ReactNode;
}

export function Button({ variant = "default", size = "md", icon, className, children, type = "button", ...rest }: ButtonProps) {
  return (
    <button type={type} className={cx("btn", variant !== "default" && variant, size === "sm" && "sm", className)} {...rest}>
      {icon && <Icon name={icon} size={size === "sm" ? 14 : 15} />}
      {children}
    </button>
  );
}

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "className" | "aria-label" | "children"> {
  /** An icon-only control needs an accessible name. */
  "aria-label": string;
  icon: string;
  size?: number;
  className?: string;
}

export function IconButton({ icon, size = 18, className, type = "button", title, ...rest }: IconButtonProps) {
  return (
    <button type={type} className={cx("icon-btn", className)} title={title ?? rest["aria-label"]} {...rest}>
      <Icon name={icon} size={size} />
    </button>
  );
}
