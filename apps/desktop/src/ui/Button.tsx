import { useId, type ButtonHTMLAttributes, type ReactNode, type Ref } from "react";
import "./Button.css";

export type ButtonVariant = "primary" | "secondary" | "ghost";

export interface ButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "disabled"> {
  variant?: ButtonVariant;
  icon?: ReactNode;
  /**
   * When set, the action is unavailable. The button stays focusable (aria-disabled) and the
   * reason is exposed to assistive technology and as a tooltip, instead of silently greying out.
   */
  unavailableReason?: string;
  /** React 19 passes refs as props: callers can return focus to the button. */
  ref?: Ref<HTMLButtonElement>;
}

export function Button({
  variant = "secondary",
  icon,
  unavailableReason,
  children,
  className,
  onClick,
  type = "button",
  ...rest
}: ButtonProps) {
  const reasonId = useId();
  const unavailable = unavailableReason !== undefined;
  return (
    <>
      <button
        {...rest}
        type={type}
        className={["btn", `btn--${variant}`, className].filter(Boolean).join(" ")}
        aria-disabled={unavailable || undefined}
        aria-describedby={unavailable ? reasonId : rest["aria-describedby"]}
        title={unavailable ? unavailableReason : rest.title}
        onClick={unavailable ? undefined : onClick}
      >
        {icon}
        <span className="btn__label">{children}</span>
      </button>
      {unavailable && (
        <span id={reasonId} className="visually-hidden">
          {unavailableReason}
        </span>
      )}
    </>
  );
}
