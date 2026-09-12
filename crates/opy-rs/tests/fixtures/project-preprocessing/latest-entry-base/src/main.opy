#!include "env/env.opy"
#!include "locales/en.opy"
#!define ENTRY_VALUE 1
#!include "composition/bootstrap.opy"
#!include "context.opy"
#!include "../env/game.opy"

rule "legacy entry":
    @Event global
    A = CONTEXT_VALUE + GAME_VALUE
