#!translations en
globalvar result
macro Player.copyAlive():
    result = self.isAlive()
rule "callable surface":
    @Event eachPlayer
    result = _("hello")
    result = ___("context", "hello")
    result = [1, 2, 3].map(lambda value: value + 1)
    result = [1, 2, 3].all()
    result = lerp(0, 10, 0.5)
    result = getSign(-1)
    eventPlayer.copyAlive()
