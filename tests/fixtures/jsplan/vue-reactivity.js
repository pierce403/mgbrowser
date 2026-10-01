// Runs the unmodified @vue/reactivity global production bundle, not a DOM app.
var reactive = VueReactivity.reactive;
var state = reactive({ count: 1 });
var doubled = VueReactivity.computed(function () { return state.count * 2; });
var observed = [];
var effect = VueReactivity.effect(function () { observed.push(doubled.value); });
state.count = 2;
if (doubled.value !== 4 || observed.join(',') !== '2,4') throw new Error('computed/effect');
VueReactivity.stop(effect);
state.count = 3;
if (observed.join(',') !== '2,4') throw new Error('effect cleanup');
var ref = VueReactivity.ref(1);
var watchValues = [];
var stop = VueReactivity.watch(ref, function (value, previous) {
  watchValues.push(previous + ':' + value);
});
ref.value = 4;
stop();
ref.value = 5;
if (watchValues.join(',') !== '1:4') throw new Error('watch cleanup');
var map = reactive(new Map());
var mapSize = VueReactivity.computed(function () { return map.size; });
map.set('a', 1);
if (mapSize.value !== 1) throw new Error('reactive Map');
var key = {};
var weak = reactive(new WeakMap());
weak.set(key, 9);
if (weak.get(key) !== 9) throw new Error('reactive WeakMap');
// Repeated short-lived scopes exercise lifetime behavior but do not measure GC.
for (var i = 0; i < 1000; i++) {
  var scope = VueReactivity.effectScope();
  scope.run(function () {
    var local = VueReactivity.ref(1);
    var run = VueReactivity.effect(function () { return local.value; });
    local.value = 2;
  });
  scope.stop();
}
var order = ['sync'];
Promise.resolve().then(function () {
  order.push('job');
  if (order.join(',') !== 'sync,job') throw new Error('Promise ordering');
  print('Test262:AsyncTestComplete');
});
true;
