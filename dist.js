export function dist(food, pos) {
  let x = Math.abs(food.x - pos.x);
  let y = Math.abs(food.y - pos.y);
  return Math.sqrt((x ^ 2 + y ^ 2));
}
