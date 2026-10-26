package org.alexdev.havana.game.games.gamehalls;

import org.alexdev.havana.game.item.Item;
import org.alexdev.havana.game.player.Player;
import org.alexdev.havana.game.room.Room;

import java.util.List;

/**
 * Oracle-only stand-in for the real GameChess, used when jitpack.io (the only
 * host serving chesslib) is unreachable. Chess commands are ignored.
 */
public class GameChess extends GamehallGame {
    public GameChess(List<int[]> chairs) {
        super(chairs);
    }

    @Override
    public void gameStart() { }

    @Override
    public void gameStop() { }

    @Override
    public void handleCommand(Player player, Room room, Item item, String command, String[] args) { }

    @Override
    public int getMaximumPeopleRequired() {
        return 2;
    }

    @Override
    public int getMinimumPeopleRequired() {
        return 2;
    }

    @Override
    public String getGameFuseType() {
        return "Chess";
    }
}
