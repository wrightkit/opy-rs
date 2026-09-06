playervar I

rule "player range":
    @Event global
    for hostPlayer.I in range(3):
        hostPlayer.I = 1
