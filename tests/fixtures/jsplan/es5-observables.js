// Authored semantic baseline: no serializer is used as the JavaScript oracle.
var effects = [];
var left = { valueOf: function () { effects.push('left'); return 2; } };
var right = { valueOf: function () { effects.push('right'); return 3; } };
if (left + right !== 5 || effects.join(',') !== 'left,right') throw 'coercion order';
var hole = [, undefined];
if (0 in hole || !(1 in hole)) throw 'array holes';
if (1 / -0 !== -Infinity || 1 / 0 !== Infinity) throw 'signed zero';
if ('\ud800'.length !== 1 || '\ud800'.charCodeAt(0) !== 55296) throw 'lone surrogate';
var n = NaN;
if (n === n) throw 'NaN equality';
var make = function (x) { return function (y) { return x + y; }; };
if (make(4)(5) !== 9) throw 'closure';
true;
