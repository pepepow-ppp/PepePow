use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use ethers::{core::rand::Rng, prelude::*, utils::keccak256};
use structopt::StructOpt;
use tokio::time::{interval, Duration};
use tokio::{
    sync::mpsc::{self},
    task::JoinHandle,
};

abigen!(
    IPOW,
    r#"[
        function mine(uint256 nonce) external
        function challenge() external view returns (uint256)
        function difficulty() external view returns (uint256)
        function balanceOf(address account) external view returns (uint256)
    ]"#,
);

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(long)]
    private_key: String,

    #[structopt(long)]
    contract_address: String,

    #[structopt(long, default_value = "10")]
    worker_count: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();
    let banner = r#"
//PPPPPPPPPPPPPPPPP                                                                                                                                         
//P::::::::::::::::P                                                                                                                                        
//P::::::PPPPPP:::::P                                                                                                                                       
//PP:::::P     P:::::P                                                                                                                                      
//  P::::P     P:::::P  eeeeeeeeeeee    ppppp   ppppppppp       eeeeeeeeeeee    ppppp   ppppppppp      ooooooooooo wwwwwww           wwwww           wwwwwww
//  P::::P     P:::::Pee::::::::::::ee  p::::ppp:::::::::p    ee::::::::::::ee  p::::ppp:::::::::p   oo:::::::::::oow:::::w         w:::::w         w:::::w 
//  P::::PPPPPP:::::Pe::::::eeeee:::::eep:::::::::::::::::p  e::::::eeeee:::::eep:::::::::::::::::p o:::::::::::::::ow:::::w       w:::::::w       w:::::w  
//  P:::::::::::::PPe::::::e     e:::::epp::::::ppppp::::::pe::::::e     e:::::epp::::::ppppp::::::po:::::ooooo:::::o w:::::w     w:::::::::w     w:::::w   
//  P::::PPPPPPPPP  e:::::::eeeee::::::e p:::::p     p:::::pe:::::::eeeee::::::e p:::::p     p:::::po::::o     o::::o  w:::::w   w:::::w:::::w   w:::::w    
//  P::::P          e:::::::::::::::::e  p:::::p     p:::::pe:::::::::::::::::e  p:::::p     p:::::po::::o     o::::o   w:::::w w:::::w w:::::w w:::::w     
//  P::::P          e::::::eeeeeeeeeee   p:::::p     p:::::pe::::::eeeeeeeeeee   p:::::p     p:::::po::::o     o::::o    w:::::w:::::w   w:::::w:::::w      
//  P::::P          e:::::::e            p:::::p    p::::::pe:::::::e            p:::::p    p::::::po::::o     o::::o     w:::::::::w     w:::::::::w       
//PP::::::PP        e::::::::e           p:::::ppppp:::::::pe::::::::e           p:::::ppppp:::::::po:::::ooooo:::::o      w:::::::w       w:::::::w        
//P::::::::P         e::::::::eeeeeeee   p::::::::::::::::p  e::::::::eeeeeeee   p::::::::::::::::p o:::::::::::::::o       w:::::w         w:::::w         
//P::::::::P          ee:::::::::::::e   p::::::::::::::pp    ee:::::::::::::e   p::::::::::::::pp   oo:::::::::::oo         w:::w           w:::w          
//PPPPPPPPPP            eeeeeeeeeeeeee   p::::::pppppppp        eeeeeeeeeeeeee   p::::::pppppppp       ooooooooooo            www             www           
//                                       p:::::p                                 p:::::p                                                                    
//                                       p:::::p                                 p:::::p                                                                    
//                                      p:::::::p                               p:::::::p                                                                   
//                                      p:::::::p                               p:::::::p                                                                   
//                                      p:::::::p                               p:::::::p                                                                   
//                                      ppppppppp                               ppppppppp                                                                                                                                                                                                                             
    "#;

    println!("{}", banner.cyan());


    let provider = Provider::<Http>::try_from("https://rpc.ankr.com/bsc")?;
    let wallet = opt.private_key.parse::<LocalWallet>()?;
    let provider = Arc::new(SignerMiddleware::new(provider, wallet));
    println!("🏅 Success init wallet");

    let contract_address: Address = opt.contract_address.parse()?;
    let contract = Arc::new(IPOW::new(contract_address, provider.clone()));

    let challenge: U256 = contract.challenge().call().await?;
    let difficulty: U256 = contract.difficulty().call().await?;

    let (result_tx, mut result_rx) = mpsc::channel::<U256>(opt.worker_count); // Adjust buffer size as needed
    let hash_counter = Arc::new(AtomicUsize::new(0));

    println!("🏆 Challenge: {}", challenge);
    println!("⛰️  Difficulty: {}", difficulty);

    let difficulty = U256::from(1) << (U256::from(256) - difficulty);
    println!("🎯 Target: {}", difficulty);

    let counter_for_timer = hash_counter.clone();
    let mut interval = interval(Duration::from_secs(1));

    let mut worker_handles: Vec<JoinHandle<()>> = Vec::new();

    for _ in 0..opt.worker_count {
        let counter = hash_counter.clone();
        let sender = result_tx.clone();
        let addr = provider.signer().address();

        let handle = tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                return mine_worker(addr, challenge, difficulty, counter);
            })
            .await
            .unwrap();

            sender.send(result).await.unwrap();
        });

        worker_handles.push(handle);
    }

    let speed_bar = ProgressBar::new(100);
    speed_bar.set_style(
        ProgressStyle::default_bar()
            .template("{prefix:.bold} {spinner:.green} {msg}")
            .unwrap()
            .progress_chars("##-"),
    );
    speed_bar.set_prefix("🚄 Speed");

    loop {
        tokio::select! {
            _ = interval.tick()=>{
                let total_hash_count = counter_for_timer.swap(0, Ordering::SeqCst);
                let hashes_per_second = total_hash_count as f64 / 1000.0;
                speed_bar.set_message(format!("Hash per second: {:.2} K/s", hashes_per_second));
            },
            nonce = result_rx.recv() => {
                if let Some(nonce) = nonce {
                    println!("✅ Find the nonce: {}", nonce);
                    let contract = contract.clone();
                    tokio::spawn(async move{
                        let result = contract.mine(nonce).send().await.unwrap().await.unwrap();
                        match result {
                            Some(tx) => {
                                println!("🙆 Successfully mined a block: {:?}", tx.transaction_hash)
                            }
                            None => {
                                println!("⚠️ Failed to mine a block");
                            }
                        }
                    });
                }
            }
        }
    }
}

fn mine_worker(
    from: Address,
    challenge: U256,
    target: U256,
    hash_counter: Arc<AtomicUsize>,
) -> U256 {
    loop {
        let mut data = Vec::new();
        let challenge_bytes = {
            let mut buf = [0u8; 32];
            challenge.to_big_endian(&mut buf);
            buf
        };
        data.extend_from_slice(&challenge_bytes);
        data.extend_from_slice(from.as_bytes());

        let nonce = rand::thread_rng().gen::<[u8; 32]>();
        let nonce_big_int = U256::from_big_endian(&nonce);

        let nonce_bytes = {
            let mut buf = [0u8; 32];
            nonce_big_int.to_big_endian(&mut buf);
            buf
        };
        data.extend_from_slice(&nonce_bytes);
        // Hash the data
        let hash = keccak256(&data);
        let hash_val = U256::from_big_endian(&hash);
        // Check if hash is less than target
        if hash_val < target {
            return nonce_big_int;
        }

        hash_counter.fetch_add(1, Ordering::SeqCst);
    }
}
