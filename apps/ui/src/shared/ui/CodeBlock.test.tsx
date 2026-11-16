import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { CodeBlock } from "./CodeBlock";
import { Markdown } from "./Markdown";

const code = (container: HTMLElement) => container.querySelector("pre code") as HTMLElement;

describe("CodeBlock", () => {
  it("shows the code plain first and coloured when the grammars arrive, with the same text all along", async () => {
    // First block of the file: the grammars chunk has not been fetched yet.
    const src = "const total = items.length; // quanti\nreturn 'ok';";
    const { container } = render(<CodeBlock code={src} lang="ts" />);
    expect(code(container).textContent).toBe(src);
    expect(container.querySelector("[class^='tok-']")).toBeNull();
    await waitFor(() => expect(container.querySelector(".tok-kw")).not.toBeNull());
    expect(code(container).textContent).toBe(src);
    expect(container.querySelector(".tok-com")?.textContent).toBe("// quanti");
    expect(container.querySelector(".tok-str")?.textContent).toBe("'ok'");
    expect(code(container).className).toBe("language-ts");
  });

  it("copies the raw text, not the coloured markup", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup({ writeToClipboard: false });
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const src = 'echo "ciao" # saluto';
    const { container } = render(<CodeBlock code={src} lang="bash" />);
    await waitFor(() => expect(container.querySelector(".tok-str")).not.toBeNull());
    await user.click(screen.getByRole("button", { name: /Copia/ }));
    expect(writeText).toHaveBeenCalledWith(src);
  });

  it("leaves a language without a grammar (or none) plain", async () => {
    const { container } = render(<><CodeBlock code="if x then y" lang="cobol" /><CodeBlock code="if x then y" /></>);
    await new Promise((r) => setTimeout(r, 50));
    expect(container.querySelector("[class^='tok-']")).toBeNull();
    expect(container.querySelectorAll("pre code")[0].className).toBe("");
    expect(container.querySelectorAll("pre code")[1].textContent).toBe("if x then y");
  });

  it("never turns code into elements: markup in the source stays text", async () => {
    const src = '<img src=x onerror="alert(1)"><script>alert(2)</script>';
    const { container } = render(<CodeBlock code={src} lang="html" />);
    await waitFor(() => expect(container.querySelector(".tok-ty")).not.toBeNull());
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("script")).toBeNull();
    expect(code(container).textContent).toBe(src);
  });

  it("keeps the block reachable from the keyboard and named for its language when it has a title", async () => {
    const { container } = render(<CodeBlock code="x = 1" lang="python" title="python" />);
    await waitFor(() => expect(container.querySelector(".tok-num")).not.toBeNull());
    expect(container.querySelector("pre")?.getAttribute("tabindex")).toBe("0");
    expect(container.querySelector(".codeblock-title")?.textContent).toBe("python");
  });

  it("follows the code while a message streams: the block is re-coloured with each new text", async () => {
    const { container, rerender } = render(<CodeBlock code="const a = 'ab" lang="js" />);
    await waitFor(() => expect(container.querySelector(".tok-str")?.textContent).toBe("'ab"));
    rerender(<CodeBlock code="const a = 'abc'; let b" lang="js" />);
    expect(container.querySelector(".tok-str")?.textContent).toBe("'abc'");
    expect(code(container).textContent).toBe("const a = 'abc'; let b");
  });
});

describe("Markdown code fences", () => {
  it("colours a fence by the first word of its info string and still labels it", async () => {
    const { container } = render(<Markdown text={"Ecco:\n\n```rust title=main.rs\nfn main() { println!(\"ciao\"); }\n```\n"} />);
    await waitFor(() => expect(container.querySelector(".tok-kw")?.textContent).toBe("fn"));
    expect(container.querySelector(".codeblock-title")?.textContent).toBe("rust");
    expect(container.querySelector("pre code")?.textContent).toBe('fn main() { println!("ciao"); }');
  });

  it("leaves a fence with no language plain", async () => {
    const { container } = render(<Markdown text={"```\nconst a = 1\n```"} />);
    await new Promise((r) => setTimeout(r, 30));
    expect(container.querySelector("[class^='tok-']")).toBeNull();
  });
});
