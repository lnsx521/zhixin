import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useT } from "../shared/i18n";

export function Modal({ title, onClose, children }: { title: string; onClose: () => void; children: ReactNode }) {
  const t = useT();
  const ref = useRef<HTMLDialogElement>(null);
  const id = useId();
  useEffect(() => { const dialog = ref.current; dialog?.showModal(); return () => dialog?.close(); }, []);
  const dialog = (
    <dialog ref={ref} className="modal" aria-labelledby={id} onCancel={(event) => { event.preventDefault(); onClose(); }}>
      <header className="modal-head"><h2 id={id}>{title}</h2><button className="btn modal-close" aria-label={t.toast.dismiss} onClick={onClose}>×</button></header>
      <div className="modal-body">{children}</div>
    </dialog>
  );
  // 服务端渲染测试没有 document，直接返回；浏览器里挂到 body 避开 .content > * 的 margin-left: 0
  return typeof document === "undefined" ? dialog : createPortal(dialog, document.body);
}

export function Disclosure({ title, hint, action, children }: { title: string; hint?: string; action?: ReactNode; children: ReactNode }) {
  return <details className="disclosure"><summary><span>{title}</span>{hint && <small>{hint}</small>}{action && <span className="disclosure-action" onClick={(event) => event.preventDefault()}>{action}</span>}</summary><div className="disclosure-body">{children}</div></details>;
}

/** 一个设置分区卡片 */
export function Section({
  title,
  description,
  action,
  wide,
  narrow,
  children,
}: {
  title: ReactNode;
  description?: ReactNode;
  action?: ReactNode;
  /** 内容复杂（表格/设备卡/长代码块）时占满整行，避免被双列挤压 */
  wide?: boolean;
  /** 内容简单的短分区：两个相邻 narrow 并排一行（必须成对标记，避免留空洞） */
  narrow?: boolean;
  children?: ReactNode;
}) {
  return (
    <section className={`section ${wide ? "wide" : ""} ${narrow ? "narrow" : ""}`}>
      {title && <div className="section-head">
        <div>
        <h2>{title}</h2>
        {description && <p className="section-desc">{description}</p>}
        </div>
        {action && <div className="section-action">{action}</div>}
      </div>}
      <div className="section-body">{children}</div>
    </section>
  );
}

/** 一行：左边标签，右边控件 */
export function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children?: ReactNode;
}) {
  const id = useId();
  return (
    <div className="row">
      <div className="row-label">
        <span id={id}>{label}</span>
        {hint && <small>{hint}</small>}
      </div>
      <div className="row-control" role="group" aria-labelledby={id}>{children}</div>
    </div>
  );
}

/** 开关 */
export function Toggle({
  checked,
  onChange,
  disabled = false,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      className={`toggle ${checked ? "on" : ""}`}
      onClick={() => onChange(!checked)}
    >
      <span className="toggle-knob" />
    </button>
  );
}

/** 数字输入 */
export function NumberField({
  value,
  onChange,
  min,
  max,
  step = 1,
  suffix,
  disabled = false,
}: {
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  suffix?: string;
  disabled?: boolean;
}) {
  return (
    <span className="number-field">
      <input
        type="number"
        disabled={disabled}
        value={value}
        min={min}
        max={max}
        step={step}
        onChange={(event) => {
          const next = Number(event.target.value);
          if (!Number.isNaN(next)) onChange(next);
        }}
      />
      {suffix && <span className="suffix">{suffix}</span>}
    </span>
  );
}

/** 字符串列表：一行一条。内部保留原始文本（含尾随换行），对外只传非空行 */
export function TextList({
  value,
  onChange,
  placeholder,
}: {
  value: string[];
  onChange: (value: string[]) => void;
  placeholder?: string;
}) {
  const [text, setText] = useState(() => value.join("\n"));
  const emitted = useRef(value.join("\n"));
  useEffect(() => {
    const incoming = value.join("\n");
    if (incoming !== emitted.current) {
      emitted.current = incoming;
      setText(incoming);
    }
  }, [value]);
  return (
    <textarea
      className="text-list"
      rows={3}
      value={text}
      placeholder={placeholder}
      onChange={(event) => {
        const next = event.target.value;
        const lines = next.split("\n").filter((line) => line.trim() !== "");
        emitted.current = lines.join("\n");
        setText(next);
        onChange(lines);
      }}
      spellCheck={false}
    />
  );
}
