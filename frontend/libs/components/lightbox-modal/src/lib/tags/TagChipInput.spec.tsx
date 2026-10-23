import { fireEvent, render, screen } from "@testing-library/react";

import { TagChipInput } from "./TagChipInput";

describe("TagChipInput", () => {
  it("commits the draft on Enter", () => {
    const onAdd = vi.fn();
    const input = renderInput(onAdd);

    fireEvent.change(input, { target: { value: "sunset" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onAdd).toHaveBeenCalledWith(["sunset"]);
  });

  // Chromium sends the Enter that confirms a Hangul syllable twice: once while
  // composing, then again once the composition has ended.
  it("waits for an IME composition to end before committing", () => {
    const onAdd = vi.fn();
    const input = renderInput(onAdd);

    fireEvent.compositionStart(input);
    fireEvent.change(input, { target: { value: "풍경" } });
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    expect(onAdd).not.toHaveBeenCalled();

    fireEvent.compositionEnd(input, { data: "경" });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onAdd).toHaveBeenCalledTimes(1);
    expect(onAdd).toHaveBeenCalledWith(["풍경"]);
  });
});

function renderInput(onAdd: (values: string[]) => void): HTMLInputElement {
  render(
    <TagChipInput
      chips={[]}
      suggestions={[]}
      onAdd={onAdd}
      onRemove={vi.fn()}
    />,
  );
  return screen.getByRole("textbox") as HTMLInputElement;
}
