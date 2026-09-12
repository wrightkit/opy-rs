#!include "env/env.opy"
#!include "locales/en.opy"
#!define ENTRY_VALUE LOBBY_SPECTATOR_SLOTS_MAIN
#!include "composition/bootstrap.opy"
#!include "../env/game.opy"

rule "legacy entry":
    @Event global
    A = GAME_VALUE
