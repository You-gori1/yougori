import { expect, it } from "vitest"
import { fitConversation, parseMarkdown } from "./model-chat"

it("parses the Markdown that chat models commonly produce", () => {
  expect(parseMarkdown("# Title\n\nSome *text*\nnext line\n\n- one\n- two\n\n1. first\n2. second\n\n> quoted\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```js\nlet x = 1\n```")).toEqual([
    { kind: "heading", level: 1, text: "Title" },
    { kind: "paragraph", text: "Some *text*\nnext line" },
    { kind: "list", ordered: false, start: 1, items: ["one", "two"] },
    { kind: "list", ordered: true, start: 1, items: ["first", "second"] },
    { kind: "quote", text: "quoted" },
    { kind: "table", head: ["a", "b"], rows: [["1", "2"]] },
    { kind: "code", lang: "js", text: "let x = 1" },
  ])
})

it("treats an unfinished code fence as code while a reply streams", () => {
  expect(parseMarkdown("Try:\n```python\nprint(1)")).toEqual([{ kind: "paragraph", text: "Try:" }, { kind: "code", lang: "python", text: "print(1)" }])
})

it("keeps the system prompt and newest turns and starts history on a user turn", () => {
  const long = "x".repeat(20000)
  const fitted = fitConversation([{ role: "system", content: "sys" }, { role: "user", content: long }, { role: "assistant", content: long }, { role: "user", content: "short" }, { role: "assistant", content: "ok" }, { role: "user", content: "latest" }])
  expect(fitted).toEqual({ messages: [{ role: "system", content: "sys" }, { role: "user", content: "short" }, { role: "assistant", content: "ok" }, { role: "user", content: "latest" }], dropped: 2 })
  expect(fitConversation([{ role: "user", content: "a" }, { role: "user", content: "b" }]).messages).toEqual([{ role: "user", content: "a\n\nb" }])
  expect(() => fitConversation([{ role: "user", content: "字".repeat(25000) }])).toThrow(/too long/)
})
