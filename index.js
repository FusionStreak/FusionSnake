// Welcome to
// __________         __    __  .__                               __
// \______   \_____ _/  |__/  |_|  |   ____   ______ ____ _____  |  | __ ____
//  |    |  _/\__  \\   __\   __\  | _/ __ \ /  ___//    \\__  \ |  |/ // __ \
//  |    |   \ / __ \|  |  |  | |  |_\  ___/ \___ \|   |  \/ __ \|    <\  ___/
//  |________/(______/__|  |__| |____/\_____>______>___|__(______/__|__\\_____>
//
// This file can be a nice home for your Battlesnake logic and helper functions.
//
// To get you started we've included code to prevent your Battlesnake from moving backwards.
// For more info see docs.battlesnake.com

import { checkMoves } from './checkMoves.js';
import { dist } from './dist.js';
import runServer from './server.js';

// info is called when you create your Battlesnake on play.battlesnake.com
// and controls your Battlesnake's appearance
// TIP: If you open your Battlesnake URL in a browser you should see this data
function info() {
  console.log("INFO");

  return {
    apiversion: "1",
    author: "FusionStreak",
    color: "#ff3d00",
    head: "smart-caterpillar",
    tail: "skinny",
  };
}

// start is called when your Battlesnake begins a game
function start(gameState) {
  console.log(`GAME START: ${gameState.game.id}`);
}

// end is called when your Battlesnake finishes a game
function end(gameState) {
  console.log("GAME OVER\n");
}

// move is called on every turn and returns your next move
// Valid moves are "up", "down", "left", or "right"
// See https://docs.battlesnake.com/api/example-move for available data
function move(gameState) {
  // The possible moves the snake can make
  const myHead = gameState.you.head;
  let possibleMoves = {
    up: { safe: true, pos: { x: myHead.x, y: myHead.y + 1 } },
    down: { safe: true, pos: { x: myHead.x, y: myHead.y - 1 } },
    left: { safe: true, pos: { x: myHead.x - 1, y: myHead.y } },
    right: { safe: true, pos: { x: myHead.x + 1, y: myHead.y } }
  };

  possibleMoves = checkMoves(gameState, possibleMoves);

  // Filter invalid moves
  const safeMoves = Object.keys(possibleMoves).filter(key => possibleMoves[key].safe);

  // Check if any moves left
  if (safeMoves.length == 0) {
    console.log(`MOVE ${gameState.turn} : No safe moves detected! Moving down`);
    return { move: "down" };
  }

  // If only one safe move, return that move
  if (safeMoves.length == 1) {
    console.log(`MOVE ${gameState.turn}: ${safeMoves[0]}`);
    return { move: safeMoves[0] };
  }


  var min = { d: undefined, move: "" }
  for (let move in possibleMoves) {
    if (possibleMoves[move].safe === false) continue;
    for (let f of gameState.board.food) {
      // If the food right on a possible move, just go there
      if (f.x === possibleMoves[move].pos.x && f.y === possibleMoves[move].pos.y) {
        console.log(`MOVE ${gameState.turn}: ${move}`);
        return { move: move };
      }
      let d = dist(f, possibleMoves[move].pos);
      if (min.d === undefined) min.d = d;
      else if (d < min.d) min = { d: d, move: move };
    }
  }
  if (min.move !== "") {
    console.log(`MOVE ${gameState.turn}: ${min.move}, d: ${min.d}`);
    return { move: min.move };
  }

  // Choose a random move from the safe moves
  const nextMove = safeMoves[Math.floor(Math.random() * safeMoves.length)];
  console.log(`MOVE ${gameState.turn}: ${nextMove}`);
  return { move: nextMove };
}

runServer({
  info: info,
  start: start,
  move: move,
  end: end
});
