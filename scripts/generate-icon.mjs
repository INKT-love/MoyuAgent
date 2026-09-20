import { existsSync } from "node:fs";

if (!existsSync("public/moyu.png")) {
  throw new Error("public/moyu.png is missing; it is the character brand avatar.");
}
console.log("Brand avatar is public/moyu.png");
