export function checkMoves(gameState, moves) {
  // Code that checks if any of the moves will result in colliding with another snake
  const myHead = gameState.you.head;
  gameState.board.snakes.forEach((snake) => {
    for (let part of snake.body) {
      if (part.y === myHead.y) {
        if (part.x === myHead.x - 1) {
          moves.left.safe = false;
        }
        else if (part.x === myHead.x + 1) {
          moves.right.safe = false;
        }
      }
      if (part.x === myHead.x) {
        if (part.y === myHead.y - 1) {
          moves.down.safe = false;
        }
        else if (part.y === myHead.y + 1) {
          moves.up.safe = false;
        }
      }
    }
  });

  // Prevent your Battlesnake from moving out of bounds
  let boardWidth = gameState.board.width;
  let boardHeight = gameState.board.height;
  for (let move in moves) {
    if ((moves[move].pos.y < 0) || (moves[move].pos.x < 0)) {
      moves[move].safe = false;
    }
    if ((moves[move].pos.y >= boardHeight) || (moves[move].pos.x >= boardWidth)) {
      moves[move].safe = false;
    }
  }

  return moves;
}
