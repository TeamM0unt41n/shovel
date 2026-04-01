/*
 * Copyright (C) 2025  Cartoone
 * Copyright (C) 2026  A. Iooss
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

// Game configuration
let startTs = 0
let tickLength = 0
window.addEventListener('configchange', e => {
  startTs = e.detail.timestampStart / 1000
  tickLength = e.detail.tickLength
  progressBarSync()
})

// Resync on focus
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') {
    progressBarSync()
  }
})

function progressBarSync () {
  if (tickLength) {
    const diffDateSec = ((Date.now() - startTs) / 1000) % tickLength
    const element = document.getElementById('progressbar-tick')
    element.style.animationDuration = `${tickLength}s`
    element.style.animationDelay = `-${diffDateSec}s`
  }
}
