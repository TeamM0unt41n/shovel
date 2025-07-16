/*
 * Copyright (C) 2025  Cartoone
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

const appElement = document.getElementById('app')
const tickLength = parseInt(appElement.dataset.tickLength, 10)
const startDate = new Date(appElement.dataset.startDate)

function progressBarSync () {
  const nowDate = new Date()
  const diffDateSec = ((nowDate - startDate) / 1000) % tickLength
  const element = document.getElementById('progressbar-tick')
  element.style.animationDuration = `${tickLength}s`
  element.style.animationDelay = `-${diffDateSec}s`
}

window.addEventListener('load', progressBarSync)
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') {
    progressBarSync()
  }
})
