/**
 * The only package a caller should import.
 *
 * <p>{@link y271727uy.stoich.api.Stoich#solve(StoichGraph)} is the whole contract: a numbered recipe graph
 * goes in, and firing counts, seed and a compressed order come back. Native loading and the
 * protobuf frame live in {@code y271727uy.stoich.internal} and are not part of this contract.
 */
package y271727uy.stoich.api;
