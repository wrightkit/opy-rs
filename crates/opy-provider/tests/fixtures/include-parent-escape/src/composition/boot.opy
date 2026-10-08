settings {
    "main": {"description": "t"},
    "gamemodes": {"ffa": {"enabledMaps": ["workshopIsland"], "enableKillCam": false}}
}

rule "boot":
    @Event global
    @Condition FLAG == false
    wait(0.1)

#!include "../env/vars.opy"
