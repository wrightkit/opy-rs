playervar values

rule "random player receiver delete":
    @Event global
    del random.choice(getAllPlayers()).values[0][0][0]
