#!define DAMAGE 2
# comments may also precede the settings block

globalvar g
playervar score

enum TeamColor:
    Red

subroutine helper

def formatScore():
    pass

macro bump(v):
    v += DAMAGE

rule "before":
    @Event global
    pass

settings {
    "main": {"modeName": "Placement"},
    "gamemodes": {"ffa": {"scoreToWin": 7}}
}

rule "count":
    @Event playerEarnedElimination
    eventPlayer.score += DAMAGE
    bump(eventPlayer.score)
