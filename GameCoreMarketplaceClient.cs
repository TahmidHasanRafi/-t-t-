using System;
using System.Collections.Generic;
using System.Linq;
using System.Numerics;
using System.Threading.Tasks;
using Solana.Unity;
using Solana.Unity.Programs.Abstract;
using Solana.Unity.Programs.Utilities;
using Solana.Unity.Rpc;
using Solana.Unity.Rpc.Builders;
using Solana.Unity.Rpc.Core.Http;
using Solana.Unity.Rpc.Core.Sockets;
using Solana.Unity.Rpc.Types;
using Solana.Unity.Wallet;
using GameCoreMarketplace;
using GameCoreMarketplace.Program;
using GameCoreMarketplace.Errors;
using GameCoreMarketplace.Accounts;
using GameCoreMarketplace.Types;

namespace GameCoreMarketplace
{
    namespace Accounts
    {
        public partial class Listing
        {
            public static ulong ACCOUNT_DISCRIMINATOR => 4186806324064035034UL;
            public static ReadOnlySpan<byte> ACCOUNT_DISCRIMINATOR_BYTES => new byte[]{218, 32, 50, 73, 43, 134, 26, 58};
            public static string ACCOUNT_DISCRIMINATOR_B58 => "dV6QTCMAagy";
            public PublicKey Asset { get; set; }

            public PublicKey Seller { get; set; }

            public ulong PriceLamports { get; set; }

            public bool Active { get; set; }

            public byte Bump { get; set; }

            public static Listing Deserialize(ReadOnlySpan<byte> _data)
            {
                int offset = 0;
                ulong accountHashValue = _data.GetU64(offset);
                offset += 8;
                if (accountHashValue != ACCOUNT_DISCRIMINATOR)
                {
                    return null;
                }

                Listing result = new Listing();
                result.Asset = _data.GetPubKey(offset);
                offset += 32;
                result.Seller = _data.GetPubKey(offset);
                offset += 32;
                result.PriceLamports = _data.GetU64(offset);
                offset += 8;
                result.Active = _data.GetBool(offset);
                offset += 1;
                result.Bump = _data.GetU8(offset);
                offset += 1;
                return result;
            }
        }

        public partial class MarketplaceConfig
        {
            public static ulong ACCOUNT_DISCRIMINATOR => 8958161820040238761UL;
            public static ReadOnlySpan<byte> ACCOUNT_DISCRIMINATOR_BYTES => new byte[]{169, 22, 247, 131, 182, 200, 81, 124};
            public static string ACCOUNT_DISCRIMINATOR_B58 => "VHPABMkHBVD";
            public PublicKey Admin { get; set; }

            public PublicKey Treasury { get; set; }

            public ulong PlatformFeeLamports { get; set; }

            public byte Bump { get; set; }

            public static MarketplaceConfig Deserialize(ReadOnlySpan<byte> _data)
            {
                int offset = 0;
                ulong accountHashValue = _data.GetU64(offset);
                offset += 8;
                if (accountHashValue != ACCOUNT_DISCRIMINATOR)
                {
                    return null;
                }

                MarketplaceConfig result = new MarketplaceConfig();
                result.Admin = _data.GetPubKey(offset);
                offset += 32;
                result.Treasury = _data.GetPubKey(offset);
                offset += 32;
                result.PlatformFeeLamports = _data.GetU64(offset);
                offset += 8;
                result.Bump = _data.GetU8(offset);
                offset += 1;
                return result;
            }
        }
    }

    namespace Errors
    {
        public enum GameCoreMarketplaceErrorKind : uint
        {
            BadPrice = 6000U,
            AlreadyListed = 6001U,
            NotListed = 6002U,
            NotSeller = 6003U,
            NotAssetOwner = 6004U,
            NameTooLong = 6005U,
            UriTooLong = 6006U,
            TypeTooLong = 6007U,
            BadRoyaltiesBps = 6008U,
            BadCreatorPercentages = 6009U
        }
    }

    namespace Types
    {
        public partial class CreatorInput
        {
            public PublicKey Address { get; set; }

            public byte Percentage { get; set; }

            public int Serialize(byte[] _data, int initialOffset)
            {
                int offset = initialOffset;
                _data.WritePubKey(Address, offset);
                offset += 32;
                _data.WriteU8(Percentage, offset);
                offset += 1;
                return offset - initialOffset;
            }

            public static int Deserialize(ReadOnlySpan<byte> _data, int initialOffset, out CreatorInput result)
            {
                int offset = initialOffset;
                result = new CreatorInput();
                result.Address = _data.GetPubKey(offset);
                offset += 32;
                result.Percentage = _data.GetU8(offset);
                offset += 1;
                return offset - initialOffset;
            }
        }
    }

    public partial class GameCoreMarketplaceClient : TransactionalBaseClient<GameCoreMarketplaceErrorKind>
    {
        public GameCoreMarketplaceClient(IRpcClient rpcClient, IStreamingRpcClient streamingRpcClient, PublicKey programId = null) : base(rpcClient, streamingRpcClient, programId ?? new PublicKey(GameCoreMarketplaceProgram.ID))
        {
        }

        public async Task<Solana.Unity.Programs.Models.ProgramAccountsResultWrapper<List<Listing>>> GetListingsAsync(string programAddress = GameCoreMarketplaceProgram.ID, Commitment commitment = Commitment.Confirmed)
        {
            var list = new List<Solana.Unity.Rpc.Models.MemCmp>{new Solana.Unity.Rpc.Models.MemCmp{Bytes = Listing.ACCOUNT_DISCRIMINATOR_B58, Offset = 0}};
            var res = await RpcClient.GetProgramAccountsAsync(programAddress, commitment, memCmpList: list);
            if (!res.WasSuccessful || !(res.Result?.Count > 0))
                return new Solana.Unity.Programs.Models.ProgramAccountsResultWrapper<List<Listing>>(res);
            List<Listing> resultingAccounts = new List<Listing>(res.Result.Count);
            resultingAccounts.AddRange(res.Result.Select(result => Listing.Deserialize(Convert.FromBase64String(result.Account.Data[0]))));
            return new Solana.Unity.Programs.Models.ProgramAccountsResultWrapper<List<Listing>>(res, resultingAccounts);
        }

        public async Task<Solana.Unity.Programs.Models.ProgramAccountsResultWrapper<List<MarketplaceConfig>>> GetMarketplaceConfigsAsync(string programAddress = GameCoreMarketplaceProgram.ID, Commitment commitment = Commitment.Confirmed)
        {
            var list = new List<Solana.Unity.Rpc.Models.MemCmp>{new Solana.Unity.Rpc.Models.MemCmp{Bytes = MarketplaceConfig.ACCOUNT_DISCRIMINATOR_B58, Offset = 0}};
            var res = await RpcClient.GetProgramAccountsAsync(programAddress, commitment, memCmpList: list);
            if (!res.WasSuccessful || !(res.Result?.Count > 0))
                return new Solana.Unity.Programs.Models.ProgramAccountsResultWrapper<List<MarketplaceConfig>>(res);
            List<MarketplaceConfig> resultingAccounts = new List<MarketplaceConfig>(res.Result.Count);
            resultingAccounts.AddRange(res.Result.Select(result => MarketplaceConfig.Deserialize(Convert.FromBase64String(result.Account.Data[0]))));
            return new Solana.Unity.Programs.Models.ProgramAccountsResultWrapper<List<MarketplaceConfig>>(res, resultingAccounts);
        }

        public async Task<Solana.Unity.Programs.Models.AccountResultWrapper<Listing>> GetListingAsync(string accountAddress, Commitment commitment = Commitment.Finalized)
        {
            var res = await RpcClient.GetAccountInfoAsync(accountAddress, commitment);
            if (!res.WasSuccessful)
                return new Solana.Unity.Programs.Models.AccountResultWrapper<Listing>(res);
            var resultingAccount = Listing.Deserialize(Convert.FromBase64String(res.Result.Value.Data[0]));
            return new Solana.Unity.Programs.Models.AccountResultWrapper<Listing>(res, resultingAccount);
        }

        public async Task<Solana.Unity.Programs.Models.AccountResultWrapper<MarketplaceConfig>> GetMarketplaceConfigAsync(string accountAddress, Commitment commitment = Commitment.Finalized)
        {
            var res = await RpcClient.GetAccountInfoAsync(accountAddress, commitment);
            if (!res.WasSuccessful)
                return new Solana.Unity.Programs.Models.AccountResultWrapper<MarketplaceConfig>(res);
            var resultingAccount = MarketplaceConfig.Deserialize(Convert.FromBase64String(res.Result.Value.Data[0]));
            return new Solana.Unity.Programs.Models.AccountResultWrapper<MarketplaceConfig>(res, resultingAccount);
        }

        public async Task<SubscriptionState> SubscribeListingAsync(string accountAddress, Action<SubscriptionState, Solana.Unity.Rpc.Messages.ResponseValue<Solana.Unity.Rpc.Models.AccountInfo>, Listing> callback, Commitment commitment = Commitment.Finalized)
        {
            SubscriptionState res = await StreamingRpcClient.SubscribeAccountInfoAsync(accountAddress, (s, e) =>
            {
                Listing parsingResult = null;
                if (e.Value?.Data?.Count > 0)
                    parsingResult = Listing.Deserialize(Convert.FromBase64String(e.Value.Data[0]));
                callback(s, e, parsingResult);
            }, commitment);
            return res;
        }

        public async Task<SubscriptionState> SubscribeMarketplaceConfigAsync(string accountAddress, Action<SubscriptionState, Solana.Unity.Rpc.Messages.ResponseValue<Solana.Unity.Rpc.Models.AccountInfo>, MarketplaceConfig> callback, Commitment commitment = Commitment.Finalized)
        {
            SubscriptionState res = await StreamingRpcClient.SubscribeAccountInfoAsync(accountAddress, (s, e) =>
            {
                MarketplaceConfig parsingResult = null;
                if (e.Value?.Data?.Count > 0)
                    parsingResult = MarketplaceConfig.Deserialize(Convert.FromBase64String(e.Value.Data[0]));
                callback(s, e, parsingResult);
            }, commitment);
            return res;
        }

        protected override Dictionary<uint, ProgramError<GameCoreMarketplaceErrorKind>> BuildErrorsDictionary()
        {
            return new Dictionary<uint, ProgramError<GameCoreMarketplaceErrorKind>>{{6000U, new ProgramError<GameCoreMarketplaceErrorKind>(GameCoreMarketplaceErrorKind.BadPrice, "Bad price.")}, {6001U, new ProgramError<GameCoreMarketplaceErrorKind>(GameCoreMarketplaceErrorKind.AlreadyListed, "Already listed.")}, {6002U, new ProgramError<GameCoreMarketplaceErrorKind>(GameCoreMarketplaceErrorKind.NotListed, "Not listed.")}, {6003U, new ProgramError<GameCoreMarketplaceErrorKind>(GameCoreMarketplaceErrorKind.NotSeller, "Not seller.")}, {6004U, new ProgramError<GameCoreMarketplaceErrorKind>(GameCoreMarketplaceErrorKind.NotAssetOwner, "Not asset owner.")}, {6005U, new ProgramError<GameCoreMarketplaceErrorKind>(GameCoreMarketplaceErrorKind.NameTooLong, "Name too long.")}, {6006U, new ProgramError<GameCoreMarketplaceErrorKind>(GameCoreMarketplaceErrorKind.UriTooLong, "URI too long.")}, {6007U, new ProgramError<GameCoreMarketplaceErrorKind>(GameCoreMarketplaceErrorKind.TypeTooLong, "Type too long.")}, {6008U, new ProgramError<GameCoreMarketplaceErrorKind>(GameCoreMarketplaceErrorKind.BadRoyaltiesBps, "Bad royalties bps.")}, {6009U, new ProgramError<GameCoreMarketplaceErrorKind>(GameCoreMarketplaceErrorKind.BadCreatorPercentages, "Bad creator percentages (must sum to 100).")}, };
        }
    }

    namespace Program
    {
        public class BuyAccounts
        {
            public PublicKey Buyer { get; set; }

            public PublicKey Config { get; set; }

            public PublicKey Treasury { get; set; }

            public PublicKey Seller { get; set; }

            public PublicKey Listing { get; set; }

            public PublicKey Asset { get; set; }

            public PublicKey CoreProgram { get; set; } = new PublicKey("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");
            public PublicKey SystemProgram { get; set; } = new PublicKey("11111111111111111111111111111111");
        }

        public class CancelAccounts
        {
            public PublicKey Seller { get; set; }

            public PublicKey Config { get; set; }

            public PublicKey Listing { get; set; }

            public PublicKey Asset { get; set; }

            public PublicKey CoreProgram { get; set; } = new PublicKey("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");
            public PublicKey SystemProgram { get; set; } = new PublicKey("11111111111111111111111111111111");
        }

        public class InitializeAccounts
        {
            public PublicKey Admin { get; set; }

            public PublicKey Treasury { get; set; }

            public PublicKey Config { get; set; }

            public PublicKey SystemProgram { get; set; } = new PublicKey("11111111111111111111111111111111");
        }

        public class ListAccounts
        {
            public PublicKey Seller { get; set; }

            public PublicKey Config { get; set; }

            public PublicKey Listing { get; set; }

            public PublicKey Asset { get; set; }

            public PublicKey CoreProgram { get; set; } = new PublicKey("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");
            public PublicKey SystemProgram { get; set; } = new PublicKey("11111111111111111111111111111111");
        }

        public class MintAssetAccounts
        {
            public PublicKey Minter { get; set; }

            public PublicKey Payer { get; set; }

            public PublicKey Asset { get; set; }

            public PublicKey CoreProgram { get; set; } = new PublicKey("CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d");
            public PublicKey SystemProgram { get; set; } = new PublicKey("11111111111111111111111111111111");
        }

        public static class GameCoreMarketplaceProgram
        {
            public const string ID = "5gGNkXZgrR9rpDuNVLXkvh1nHKCrZuKZhWz4eGwkcwM2";
            public static Solana.Unity.Rpc.Models.TransactionInstruction Buy(BuyAccounts accounts, PublicKey programId = null)
            {
                programId ??= new(ID);
                List<Solana.Unity.Rpc.Models.AccountMeta> keys = new()
                {Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Buyer, true), Solana.Unity.Rpc.Models.AccountMeta.ReadOnly(accounts.Config, false), Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Treasury, false), Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Seller, false), Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Listing, false), Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Asset, false), Solana.Unity.Rpc.Models.AccountMeta.ReadOnly(accounts.CoreProgram, false), Solana.Unity.Rpc.Models.AccountMeta.ReadOnly(accounts.SystemProgram, false)};
                byte[] _data = new byte[1200];
                int offset = 0;
                _data.WriteU64(16927863322537952870UL, offset);
                offset += 8;
                byte[] resultData = new byte[offset];
                Array.Copy(_data, resultData, offset);
                return new Solana.Unity.Rpc.Models.TransactionInstruction{Keys = keys, ProgramId = programId.KeyBytes, Data = resultData};
            }

            public static Solana.Unity.Rpc.Models.TransactionInstruction Cancel(CancelAccounts accounts, PublicKey programId = null)
            {
                programId ??= new(ID);
                List<Solana.Unity.Rpc.Models.AccountMeta> keys = new()
                {Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Seller, true), Solana.Unity.Rpc.Models.AccountMeta.ReadOnly(accounts.Config, false), Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Listing, false), Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Asset, false), Solana.Unity.Rpc.Models.AccountMeta.ReadOnly(accounts.CoreProgram, false), Solana.Unity.Rpc.Models.AccountMeta.ReadOnly(accounts.SystemProgram, false)};
                byte[] _data = new byte[1200];
                int offset = 0;
                _data.WriteU64(13753127788127181800UL, offset);
                offset += 8;
                byte[] resultData = new byte[offset];
                Array.Copy(_data, resultData, offset);
                return new Solana.Unity.Rpc.Models.TransactionInstruction{Keys = keys, ProgramId = programId.KeyBytes, Data = resultData};
            }

            public static Solana.Unity.Rpc.Models.TransactionInstruction Initialize(InitializeAccounts accounts, PublicKey treasury, PublicKey programId = null)
            {
                programId ??= new(ID);
                List<Solana.Unity.Rpc.Models.AccountMeta> keys = new()
                {Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Admin, true), Solana.Unity.Rpc.Models.AccountMeta.ReadOnly(accounts.Treasury, false), Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Config, false), Solana.Unity.Rpc.Models.AccountMeta.ReadOnly(accounts.SystemProgram, false)};
                byte[] _data = new byte[1200];
                int offset = 0;
                _data.WriteU64(17121445590508351407UL, offset);
                offset += 8;
                _data.WritePubKey(treasury, offset);
                offset += 32;
                byte[] resultData = new byte[offset];
                Array.Copy(_data, resultData, offset);
                return new Solana.Unity.Rpc.Models.TransactionInstruction{Keys = keys, ProgramId = programId.KeyBytes, Data = resultData};
            }

            public static Solana.Unity.Rpc.Models.TransactionInstruction List(ListAccounts accounts, ulong price_lamports, PublicKey programId = null)
            {
                programId ??= new(ID);
                List<Solana.Unity.Rpc.Models.AccountMeta> keys = new()
                {Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Seller, true), Solana.Unity.Rpc.Models.AccountMeta.ReadOnly(accounts.Config, false), Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Listing, false), Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Asset, false), Solana.Unity.Rpc.Models.AccountMeta.ReadOnly(accounts.CoreProgram, false), Solana.Unity.Rpc.Models.AccountMeta.ReadOnly(accounts.SystemProgram, false)};
                byte[] _data = new byte[1200];
                int offset = 0;
                _data.WriteU64(2775388424495017526UL, offset);
                offset += 8;
                _data.WriteU64(price_lamports, offset);
                offset += 8;
                byte[] resultData = new byte[offset];
                Array.Copy(_data, resultData, offset);
                return new Solana.Unity.Rpc.Models.TransactionInstruction{Keys = keys, ProgramId = programId.KeyBytes, Data = resultData};
            }

            public static Solana.Unity.Rpc.Models.TransactionInstruction MintAsset(MintAssetAccounts accounts, string name, string uri, string optional_type, ushort? royalties_bps, CreatorInput[] creators, PublicKey programId = null)
            {
                programId ??= new(ID);
                List<Solana.Unity.Rpc.Models.AccountMeta> keys = new()
                {Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Minter, true), Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Payer, true), Solana.Unity.Rpc.Models.AccountMeta.Writable(accounts.Asset, false), Solana.Unity.Rpc.Models.AccountMeta.ReadOnly(accounts.CoreProgram, false), Solana.Unity.Rpc.Models.AccountMeta.ReadOnly(accounts.SystemProgram, false)};
                byte[] _data = new byte[1200];
                int offset = 0;
                _data.WriteU64(8532344615109635924UL, offset);
                offset += 8;
                offset += _data.WriteBorshString(name, offset);
                offset += _data.WriteBorshString(uri, offset);
                if (optional_type != null)
                {
                    _data.WriteU8(1, offset);
                    offset += 1;
                    offset += _data.WriteBorshString(optional_type, offset);
                }
                else
                {
                    _data.WriteU8(0, offset);
                    offset += 1;
                }

                if (royalties_bps != null)
                {
                    _data.WriteU8(1, offset);
                    offset += 1;
                    _data.WriteU16(royalties_bps.Value, offset);
                    offset += 2;
                }
                else
                {
                    _data.WriteU8(0, offset);
                    offset += 1;
                }

                _data.WriteS32(creators.Length, offset);
                offset += 4;
                foreach (var creatorsElement in creators)
                {
                    offset += creatorsElement.Serialize(_data, offset);
                }

                byte[] resultData = new byte[offset];
                Array.Copy(_data, resultData, offset);
                return new Solana.Unity.Rpc.Models.TransactionInstruction{Keys = keys, ProgramId = programId.KeyBytes, Data = resultData};
            }
        }
    }
}