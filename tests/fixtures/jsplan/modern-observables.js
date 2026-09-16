const trace = [];
let count = 0;
const target = { n: 2 };
const proxy = new Proxy(target, {
  get(object, key, receiver) { trace.push(key); return Reflect.get(object, key, receiver); }
});
const add = value => value + proxy.n;
if (add(3) !== 5 || trace.join(',') !== 'n') throw new Error('proxy / arrow');
const map = new Map([['one', 1], ['two', 2]]);
for (const [key, value] of map) count += value;
if (count !== 3) throw new Error('iteration');
const weak = new WeakMap();
weak.set(target, 7);
if (weak.get(target) !== 7) throw new Error('WeakMap identity');
class Box { constructor(value) { this.value = value; } get() { return this.value; } }
if (new Box(11).get() !== 11) throw new Error('class');
true;
