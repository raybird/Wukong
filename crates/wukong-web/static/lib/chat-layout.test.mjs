import test from 'node:test';
import assert from 'node:assert/strict';

import { waitForStableScrollHeight } from './chat-layout.mjs';

test('waitForStableScrollHeight waits until scroll height is stable', async () => {
  const heights = [100, 140, 180, 180, 180];
  let frameCount = 0;
  const log = {
    get scrollHeight() {
      return heights[Math.min(frameCount, heights.length - 1)];
    },
  };
  const nextFrame = async () => {
    frameCount += 1;
  };

  await waitForStableScrollHeight(log, nextFrame, { stableFrames: 2, maxFrames: 8 });

  assert.equal(frameCount, 4);
});
