globalvar g

rule "context player components":
    @Event eachPlayer
    g = eventPlayer.x
    g = eventPlayer.y
    g = eventPlayer.z
