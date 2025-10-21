import { useEffect } from "react";

interface Transaction {
    title: string;
    hash?: string;
    timestamp: number;
}

interface TransactionListProps {
    transactions: Transaction[];
    setTransactions: (callback: (prev: Transaction[]) => Transaction[]) => void;
}

export const TransactionList = ({ transactions, setTransactions }: TransactionListProps) => {
    useEffect(() => {
        const timeout = setTimeout(() => {
            setTransactions((prev) => prev.filter((tx) => Date.now() - tx.timestamp < 3000));
        }, 1000);

        return () => clearTimeout(timeout);
    }, [transactions, setTransactions]);

    if (transactions.length === 0) {
        return null;
    }

    return (
        <div className="transaction-list">
            {transactions.map((tx) => (
                <div key={tx.timestamp} className="transaction-list__item">
                    <div className="transaction-list__title">{tx.title}</div>
                    {tx.hash ? <div className="transaction-list__hash">{tx.hash}</div> : null}
                </div>
            ))}
        </div>
    );
};
