#!include "env/env.opy"
#!include "locales/en.opy"
#!define ENTRY_VALUE 1
#!include "composition/bootstrap.opy"
#!include "../env/game.opy"

rule "legacy entry":
    @Event global
    A = GAME_VALUE
