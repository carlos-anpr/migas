import { createRequire } from 'node:module';
import assert from 'node:assert/strict';
import test from 'node:test';

const require = createRequire(import.meta.url);
const cargado = require('../detector.js');
// El `package.json` de la raíz declara `"type": "module"`: Node ejecuta este `.js` como ESM
// y no crea `module`. En un contexto CommonJS el mismo fichero publica `module.exports`;
// en ambos casos está `globalThis.migasDetector`.
const api =
  cargado && typeof cargado.crearDetector === 'function'
    ? cargado
    : globalThis.migasDetector;
const { crearDetector } = api;

function reloj(ms = 330) {
  let t = 0;
  const detector = crearDetector({ ms, ahora: () => t });
  return {
    detector,
    en(valor) {
      t = valor;
      return detector;
    },
  };
}

function tap(reloj, inicio, duracion) {
  reloj.en(inicio);
  reloj.detector.evento('CtrlAbajo');
  reloj.en(inicio + duracion);
  return reloj.detector.evento('CtrlArriba');
}

test('expone la fábrica en globalThis y en module.exports cuando existe', () => {
  assert.equal(typeof crearDetector, 'function');
  assert.equal(typeof globalThis.migasDetector?.crearDetector, 'function');
});

test('doble pulsación básica: dispara al soltar la segunda', () => {
  const r = reloj();
  assert.equal(tap(r, 0, 80), false);
  r.en(200);
  assert.equal(r.detector.evento('CtrlAbajo'), false);
  r.en(250);
  assert.equal(r.detector.evento('CtrlArriba'), true);
});

test('un tap lento no cuenta', () => {
  const r = reloj();
  assert.equal(tap(r, 0, 300), false);
  r.en(400);
  assert.equal(r.detector.evento('CtrlAbajo'), false);
  r.en(450);
  assert.equal(r.detector.evento('CtrlArriba'), false);
});

test('la segunda pulsación tarde no cuenta', () => {
  const r = reloj(200);
  assert.equal(tap(r, 0, 50), false);
  // 200 ms justos desde la suelta: no es menor que el máximo.
  r.en(250);
  assert.equal(r.detector.evento('CtrlAbajo'), false);
  r.en(300);
  assert.equal(r.detector.evento('CtrlArriba'), false);
});

test('el intervalo de la segunda pulsación es configurable', () => {
  const corto = reloj(100);
  assert.equal(tap(corto, 0, 50), false);
  corto.en(200);
  assert.equal(corto.detector.evento('CtrlAbajo'), false);
  corto.en(250);
  assert.equal(corto.detector.evento('CtrlArriba'), false);

  const largo = reloj(500);
  assert.equal(tap(largo, 0, 50), false);
  largo.en(200);
  assert.equal(largo.detector.evento('CtrlAbajo'), false);
  largo.en(250);
  assert.equal(largo.detector.evento('CtrlArriba'), true);
});

test('Ctrl+C en la segunda pulsación no dispara', () => {
  const r = reloj();
  assert.equal(tap(r, 0, 50), false);
  r.en(150);
  assert.equal(r.detector.evento('CtrlAbajo'), false);
  r.en(170);
  r.detector.evento('OtraTecla');
  r.en(200);
  assert.equal(r.detector.evento('CtrlArriba'), false);
});

test('una segunda pulsación larga no dispara', () => {
  const r = reloj();
  assert.equal(tap(r, 0, 50), false);
  r.en(150);
  assert.equal(r.detector.evento('CtrlAbajo'), false);
  r.en(800);
  assert.equal(r.detector.evento('CtrlArriba'), false);
});

test('la triple pulsación produce una sola detección', () => {
  const r = reloj();
  assert.equal(tap(r, 0, 50), false);
  r.en(150);
  assert.equal(r.detector.evento('CtrlAbajo'), false);
  r.en(200);
  assert.equal(r.detector.evento('CtrlArriba'), true);
  // Tercera dentro de la ventana de bloqueo: no vuelve a disparar.
  assert.equal(tap(r, 250, 50), false);
});

test('el autorepeat de Control se ignora', () => {
  const r = reloj();
  r.en(0);
  assert.equal(r.detector.evento('CtrlAbajo'), false);
  r.en(30);
  assert.equal(r.detector.evento('CtrlAbajo'), false);
  r.en(60);
  assert.equal(r.detector.evento('CtrlArriba'), false);
  r.en(150);
  assert.equal(r.detector.evento('CtrlAbajo'), false);
  r.en(200);
  assert.equal(r.detector.evento('CtrlArriba'), true);
});

test('otra tecla o rueda entre pulsaciones anula', () => {
  const tecla = reloj();
  assert.equal(tap(tecla, 0, 50), false);
  tecla.en(100);
  tecla.detector.evento('OtraTecla');
  tecla.en(150);
  assert.equal(tecla.detector.evento('CtrlAbajo'), false);

  const rueda = reloj();
  assert.equal(tap(rueda, 0, 50), false);
  rueda.en(100);
  rueda.detector.evento('Rueda');
  rueda.en(150);
  assert.equal(rueda.detector.evento('CtrlAbajo'), false);
});

test('los métodos de teclado traducen keydown y keyup', () => {
  const r = reloj();
  r.en(0);
  r.detector.keydown({ key: 'Control', repeat: false });
  r.en(50);
  r.detector.keyup({ key: 'Control' });
  r.en(150);
  r.detector.keydown({ key: 'Control', repeat: false });
  r.en(200);
  assert.equal(r.detector.keyup({ key: 'Control' }), true);
});
