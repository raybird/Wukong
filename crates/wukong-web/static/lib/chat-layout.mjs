export async function waitForStableScrollHeight(
  element,
  nextFrame,
  { stableFrames = 3, maxFrames = 12 } = {}
) {
  let previousHeight = element.scrollHeight;
  let stableCount = 0;

  for (let frame = 0; frame < maxFrames; frame += 1) {
    await nextFrame();
    const currentHeight = element.scrollHeight;
    if (currentHeight === previousHeight) {
      stableCount += 1;
      if (stableCount >= stableFrames) return;
    } else {
      previousHeight = currentHeight;
      stableCount = 0;
    }
  }
}
