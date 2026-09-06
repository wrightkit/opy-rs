# Receiver/member call regression (issue #104).
#
# Real-world `.opy` receiver-call forms derived from the overpy-meipocalypse
# corpus (see fixture.json provenance): `eventPlayer.<builtin>(...)` actions
# and value methods used in conditions and as call arguments, plus a player
# variable receiver. Every method here is a Workshop action/value function
# within the declared supported surface; unsupported member forms (for
# example enum-gated methods such as setInvisibility/setStatusEffect) stay
# out of this fixture and fail with structured diagnostics.

globalvar target

rule "move speed":
	@Event eachPlayer
	@Condition eventPlayer.isAlive()
	eventPlayer.setMoveSpeed(100)

rule "combat teleport":
	@Event eachPlayer
	eventPlayer.setMaxHealth(200)
	eventPlayer.setHealth(200)
	eventPlayer.teleport(eventPlayer.getPosition())

rule "utility adjustments":
	@Event eachPlayer
	eventPlayer.setAimSpeed(150)
	eventPlayer.setGravity(100)
	eventPlayer.setDamageDealt(50)
	eventPlayer.setDamageReceived(50)

rule "conditional support":
	@Event eachPlayer
	@Condition eventPlayer.getHealth() < 50
	@Condition eventPlayer.hasSpawned()
	eventPlayer.setMoveSpeed(75)
	eventPlayer.setUltCharge(100)

rule "player receiver":
	@Event eachPlayer
	target = eventPlayer
	target.setMoveSpeed(50)
